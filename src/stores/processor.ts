import { defineStore } from 'pinia'
import { invoke, Channel } from '@tauri-apps/api/core'
import { isTauri } from '@tauri-apps/api/core'
import { useProcessor } from '../composables/useProcessor'
import type {
  MediaInfo,
  UseCase,
  RecommendResult,
  ProcessProgress,
  ProcessResult,
  ProcessOptions,
  AppSettings,
} from '../types'

// ProcessResult is also used by save_from_preview invoke — re-export for clarity
export type { ProcessResult }

// Module-level debounce timer (outside reactive state)
let previewDebounceTimer: ReturnType<typeof setTimeout> | null = null

// ── Types ──────────────────────────────────────────────────────────────────

export type Phase =
  | 'idle'
  | 'analyzing'
  | 'recommend'
  | 'processing'
  | 'done'
  | 'error'
  | 'setup'
  | 'downloading'

/** How the user wants to process the file */
export type ProcessMode = 'compress' | 'convert' | 'size-limit'

/** Quality/degradation level */
export type QualityPreset = 'high' | 'balanced' | 'small' | 'tiny'

/**
 * Quality values per media type.
 * image  : 0-100 scale (higher = better)
 * video  : CRF value  (lower = better; used directly by the backend)
 * audio  : bitrate in kbps (passed as target.quality to build_audio_args)
 */
const QUALITY_VALUES: Record<QualityPreset, { image: number; video: number; audio: number }> = {
  high:     { image: 92, video: 18, audio: 192 },
  balanced: { image: 78, video: 23, audio: 128 },
  small:    { image: 60, video: 28, audio: 96  },
  tiny:     { image: 40, video: 35, audio: 64  },
}

export interface FfmpegStatus {
  available: boolean
  hasWebp: boolean
  hasAvif: boolean
  hasAvifAlpha: boolean
  path: string
  isManaged: boolean
  downloadSupported: boolean
}

export interface AvifencStatus {
  available: boolean
  avifdecAvailable: boolean
  path: string | null
  version: string | null
  installSupported: boolean
}

export interface PreviewState {
  path: string
  outputSize: number
  format: string
}

export interface DownloadProgress {
  phase: string
  percent: number
  message: string
}

// ── Store ──────────────────────────────────────────────────────────────────

export const useProcessorStore = defineStore('processor', {
  state: () => ({
    phase: 'idle' as Phase,
    file: null as File | string | null,
    mediaInfo: null as MediaInfo | null,
    recommendResult: null as RecommendResult | null,

    // Processing mode & options (replaces selectedUseCase / discordTier)
    processMode: 'compress' as ProcessMode,
    qualityPreset: 'balanced' as QualityPreset,
    selectedFormat: null as string | null,
    selectedCodec: null as string | null,
    targetSizeBytes: null as number | null,

    progress: null as ProcessProgress | null,
    result: null as ProcessResult | null,
    error: null as string | null,
    currentFileId: null as string | null,
    settings: { allowCpuAV1: false } as AppSettings,

    ffmpegStatus: null as FfmpegStatus | null,
    avifencStatus: null as AvifencStatus | null,
    hwAccel: [] as string[],
    previewState: null as PreviewState | null,
    previewLoading: false,
    previewJobId: null as string | null,

    dlProgress: null as DownloadProgress | null,
    dlLog: [] as string[],
  }),

  getters: {
    fileName(state): string {
      if (!state.file) return ''
      if (typeof state.file === 'string') return state.file.split('/').pop() ?? state.file
      return (state.file as File).name
    },

    /** UseCase derived from processMode for compatibility with the backend */
    effectiveUseCase(state): UseCase {
      switch (state.processMode) {
        case 'compress':   return { type: 'storage' }
        case 'convert':    return { type: 'compat', target: 'universal' }
        case 'size-limit': return { type: 'custom' }
      }
    },
  },

  actions: {
    /** Build the full ProcessOptions from current state. Used by execute() and triggerPreview(). */
    buildProcessOptions(): ProcessOptions {
      const type = this.mediaInfo?.type ?? 'image'
      const qv   = QUALITY_VALUES[this.qualityPreset]
      const quality = qv[type as keyof typeof qv] ?? qv.image

      // For compress mode, preserve the input format (re-encode same container).
      let format = this.selectedFormat ?? undefined
      if (!format && this.processMode === 'compress') {
        const ext = this.fileName.split('.').pop()?.toLowerCase() ?? ''
        if (ext) format = ext === 'jpeg' ? 'jpg' : ext === 'm4a' ? 'aac' : ext
      }

      return {
        useCase: this.effectiveUseCase,
        format,
        target: {
          quality,
          maxSizeBytes: this.processMode === 'size-limit'
            ? (this.targetSizeBytes ?? undefined)
            : undefined,
        },
        codec: this.selectedCodec ?? undefined,
        description: `mode:${this.processMode} q:${quality}`,
      }
    },

    // ── Startup ──────────────────────────────────────────────────────────

    async initFfmpeg() {
      if (!isTauri()) return
      const [status, avifStatus, hwAccel] = await Promise.all([
        invoke<FfmpegStatus>('check_ffmpeg'),
        invoke<AvifencStatus>('check_avifenc'),
        invoke<string[]>('detect_hw_accel'),
      ])
      this.ffmpegStatus = status
      this.avifencStatus = avifStatus
      this.hwAccel = hwAccel
      if (!status.available) this.phase = 'setup'
    },

    async downloadFfmpeg() {
      this.phase = 'downloading'
      this.dlProgress = null
      this.dlLog = []
      this.error = null

      const channel = new Channel<DownloadProgress>()
      channel.onmessage = (p) => {
        this.dlProgress = p
        if (p.message) {
          for (const line of p.message.split('\n')) {
            if (line.trim()) this.dlLog.push(line)
          }
          if (this.dlLog.length > 1000) this.dlLog.splice(0, this.dlLog.length - 1000)
        }
      }

      try {
        await invoke('download_ffmpeg', { onProgress: channel })
        this.ffmpegStatus = await invoke<FfmpegStatus>('check_ffmpeg')
        this.phase = 'idle'
      } catch (e: unknown) {
        this.error = String(e)
        this.phase = 'setup'
      }
    },

    skipSetup() { this.phase = 'idle' },

    // ── File processing ───────────────────────────────────────────────────

    async loadFile(file: File | string) {
      this.file = file
      this.phase = 'analyzing'
      this.error = null
      this.previewState = null
      this.previewLoading = false
      const proc = useProcessor()
      try {
        this.mediaInfo = await proc.analyze(file)
        this.recommendResult = await proc.recommend(
          this.mediaInfo, this.effectiveUseCase, this.settings,
        )
        this.phase = 'recommend'
        if (this.mediaInfo.type === 'image') this.scheduleTriggerPreview()
      } catch (e: unknown) {
        this.error = String(e)
        this.phase = 'error'
      }
    },

    async execute() {
      if (!this.file || !this.mediaInfo) return
      this.phase = 'processing'
      this.progress = null
      this.result = null

      // Images with a valid preview: just copy the temp file instead of re-encoding.
      // previewState is cleared on every settings change, so if it's set it always
      // matches the current buildProcessOptions() output.
      const preview = this.previewState
      if (preview && this.mediaInfo.type === 'image' && isTauri()) {
        const filePath = typeof this.file === 'string'
          ? this.file
          : (this.file as File & { path?: string })?.path ?? ''
        if (filePath) {
          try {
            this.result = await invoke<ProcessResult>('save_from_preview', {
              previewPath: preview.path,
              inputPath: filePath,
              format: preview.format,
            })
            this.previewState = null
            this.phase = this.result.success ? 'done' : 'error'
            if (!this.result.success) this.error = this.result.error ?? '保存失敗'
          } catch (e: unknown) {
            this.error = String(e)
            this.phase = 'error'
          }
          return
        }
      }

      const proc = useProcessor()
      const fileId = `${Date.now()}-${Math.random().toString(36).slice(2)}`
      this.currentFileId = fileId
      try {
        this.result = await proc.process(
          this.file,
          this.buildProcessOptions(),
          (p) => { this.progress = p },
        )
        if (this.result.success) {
          this.phase = 'done'
        } else {
          this.error = this.result.error ?? '処理に失敗しました'
          this.phase = 'error'
        }
      } catch (e: unknown) {
        if (String(e) === 'Cancelled') {
          this.phase = 'idle'
        } else {
          this.error = String(e)
          this.phase = 'error'
        }
      }
    },

    async cancel() {
      if (!this.currentFileId) return
      await useProcessor().cancel(this.currentFileId)
    },

    // ── Setters (each triggers a preview refresh for images) ─────────────

    setProcessMode(mode: ProcessMode) {
      this.processMode = mode
      // Reset format/codec when switching modes to avoid stale selection
      if (mode === 'compress') {
        this.selectedFormat = null
        this.selectedCodec = null
      }
      this.previewState = null
      if (this.mediaInfo?.type === 'image') this.scheduleTriggerPreview()
    },

    setQualityPreset(preset: QualityPreset) {
      this.qualityPreset = preset
      this.previewState = null
      if (this.mediaInfo?.type === 'image') this.scheduleTriggerPreview()
    },

    setFormat(fmt: string | null) {
      this.selectedFormat = fmt
      this.previewState = null
      if (this.mediaInfo?.type === 'image') this.scheduleTriggerPreview()
    },

    setSelectedCodec(codec: string | null) {
      this.selectedCodec = codec
      this.previewState = null
      if (this.mediaInfo?.type === 'image') this.scheduleTriggerPreview()
    },

    setTargetSize(bytes: number | null) {
      this.targetSizeBytes = bytes
    },

    reset() {
      if (previewDebounceTimer) { clearTimeout(previewDebounceTimer); previewDebounceTimer = null }
      if (isTauri()) invoke('cleanup_preview_cache').catch(() => {})
      this.$reset()
    },

    // ── Preview ───────────────────────────────────────────────────────────

    scheduleTriggerPreview() {
      if (previewDebounceTimer) clearTimeout(previewDebounceTimer)
      previewDebounceTimer = setTimeout(() => this.triggerPreview(), 350)
    },

    async triggerPreview() {
      if (!isTauri()) return
      if (!this.mediaInfo || this.mediaInfo.type !== 'image') return
      const filePath = typeof this.file === 'string'
        ? this.file
        : (this.file as File & { path?: string })?.path ?? null
      if (!filePath) return

      if (this.previewJobId) {
        invoke('cancel_process', { fileId: this.previewJobId }).catch(() => {})
      }

      const jobId = `${Date.now()}-${Math.random().toString(36).slice(2)}`
      this.previewJobId = jobId
      this.previewLoading = true

      const options = this.buildProcessOptions()

      try {
        const result = await invoke<{ path: string; outputSize: number }>(
          'preview_image',
          { previewId: jobId, input: filePath, options },
        )
        if (this.previewJobId === jobId) {
          this.previewState = {
            path: result.path,
            outputSize: result.outputSize,
            format: options.format ?? this.fileName.split('.').pop()?.toLowerCase() ?? 'jpg',
          }
          this.previewLoading = false
        }
      } catch (e) {
        if (this.previewJobId === jobId && String(e) !== 'Cancelled') {
          this.previewLoading = false
        }
      }
    },
  },
})
