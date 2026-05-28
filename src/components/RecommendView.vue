<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { useProcessorStore, type ProcessMode, type QualityPreset } from '../stores/processor'
import { formatBytes, reductionPercent } from '../lib/format'
import FilePreview from './FilePreview.vue'

const store = useProcessorStore()
const info  = computed(() => store.mediaInfo!)

// ── Input characteristics ──────────────────────────────────────────────────

const inputExt = computed(() =>
  store.fileName.split('.').pop()?.toLowerCase() ?? ''
)

const hasAlpha = computed(() => info.value?.hasAlpha === true)

const isCompatFirst = computed(() => {
  const ext  = inputExt.value
  const mime = info.value?.mimeType ?? ''
  return ['heic', 'heif', 'avif'].includes(ext) ||
         ['image/heic', 'image/heif', 'image/avif'].includes(mime)
})

// Auto-select mode and format when a new file is loaded
watch(info, (val) => {
  if (!val) return
  if (isCompatFirst.value || hasAlpha.value) {
    store.setProcessMode('convert')
    store.setFormat(
      hasAlpha.value && store.avifencStatus?.available ? 'avif' : 'png',
    )
  }
}, { immediate: true })

// ── Mode ───────────────────────────────────────────────────────────────────

const mode = computed(() => store.processMode)

const MODES: { value: ProcessMode; icon: string; label: string; desc: string }[] = [
  {
    value: 'compress',
    icon: '📉',
    label: '圧縮する',
    desc: '同じ形式のまま品質を下げてサイズを削減',
  },
  {
    value: 'convert',
    icon: '🔄',
    label: '形式を変換',
    desc: '別の形式・コーデックに変換',
  },
  {
    value: 'size-limit',
    icon: '🎯',
    label: 'サイズを指定',
    desc: '動画向け：目標ファイルサイズを指定',
  },
]

function selectMode(m: ProcessMode) {
  store.setProcessMode(m)
  if (m === 'size-limit' && !store.targetSizeBytes) {
    store.setTargetSize(10 * 1024 * 1024) // default 10 MB
    targetSizeMb.value = 10
  }
}

// ── Format options ─────────────────────────────────────────────────────────

const ALPHA_CAPABLE = ['png', 'webp', 'avif']

const formatOptions = computed(() => {
  const type = info.value?.type
  const ext  = inputExt.value
  const mime = info.value?.mimeType ?? ''
  const caps    = store.ffmpegStatus
  const avifenc = store.avifencStatus
  const alpha   = hasAlpha.value

  if (type === 'image') {
    const srcExt = ext === 'jpeg' ? 'jpg' : ext
    const isAvif = srcExt === 'avif' || mime === 'image/avif'
    const canWebp      = caps ? caps.hasWebp : true
    const canAvif      = (caps ? caps.hasAvif : true) || (avifenc?.available ?? false)
    const canAvifAlpha = (caps ? caps.hasAvifAlpha : true) || (avifenc?.available ?? false)
    const avifNote     = alpha && avifenc?.available
      ? '透過対応 (libavif)'
      : alpha && canAvifAlpha ? '透過対応' : '最小サイズ'

    return [
      { value: 'jpg',  label: 'JPEG',  note: alpha ? '透過を失います' : '汎用・互換性最高', warn: alpha },
      { value: 'png',  label: 'PNG',   note: 'ロスレス（品質設定は無効）', warn: false },
      canWebp ? { value: 'webp', label: 'WebP', note: 'バランス', warn: false } : null,
      canAvif && (!alpha || canAvifAlpha)
        ? { value: 'avif', label: 'AVIF', note: avifNote, warn: false }
        : null,
    ].filter((f): f is NonNullable<typeof f> => f !== null && f.value !== srcExt && !(isAvif && ['webp','avif'].includes(f.value)))
  }

  // Video format options: only used for audio in convert mode (not video —
  // video uses bundled format+codec options below)
  if (type === 'audio') {
    const srcExt = ['m4a'].includes(ext) ? 'aac' : ext
    return [
      { value: 'mp3',  label: 'MP3',  note: '汎用',           warn: false },
      { value: 'aac',  label: 'AAC',  note: '互換性高',       warn: false },
      { value: 'opus', label: 'Opus', note: '高品質・小サイズ', warn: false },
      { value: 'flac', label: 'FLAC', note: 'ロスレス',       warn: false },
    ].filter(f => f.value !== srcExt)
  }

  return []
})

// ── Hardware acceleration detection ──────────────────────────────────────

const hasH264HW = computed(() =>
  store.hwAccel.some(h => ['nvenc_h264', 'amf_h264', 'qsv_h264', 'videotoolbox_h264'].includes(h)),
)
const hasHEVCHW = computed(() =>
  store.hwAccel.some(h => ['nvenc_hevc', 'amf_hevc', 'qsv_hevc', 'videotoolbox_hevc'].includes(h)),
)
const hasAV1HW = computed(() =>
  store.hwAccel.some(h => ['nvenc_av1', 'amf_av1', 'qsv_av1'].includes(h)),
)

// ── Video: separate container + codec selection ───────────────────────────

const VIDEO_CONTAINERS = [
  { value: 'mp4',  label: 'MP4',  note: '互換性最高' },
  { value: 'mkv',  label: 'MKV',  note: '高品質保存' },
  { value: 'webm', label: 'WebM', note: 'Web向け' },
] as const

const VIDEO_CODECS = [
  { value: 'h264', label: 'H.264', note: '高速・高互換' },
  { value: 'h265', label: 'H.265', note: '高圧縮' },
  { value: 'av1',  label: 'AV1',   note: '次世代' },
] as const

function isContainerDisabled(container: string): boolean {
  const codec = store.selectedCodec
  if (container === 'webm') {
    if (codec === 'h264' || codec === 'h265') return true
    // HW AV1 encoders (nvenc/amf/qsv) cannot mux into WebM
    if (codec === 'av1' && hasAV1HW.value) return true
  }
  return false
}

function isCodecDisabled(codec: string): boolean {
  return codec === 'av1' && !hasAV1HW.value && !store.settings.allowCpuAV1
}

function getCodecHwBadge(codec: string): string | null {
  const hw = store.hwAccel
  const map: Record<string, string[]> = {
    h264: ['nvenc_h264', 'amf_h264', 'qsv_h264', 'videotoolbox_h264'],
    h265: ['nvenc_hevc', 'amf_hevc', 'qsv_hevc', 'videotoolbox_hevc'],
    av1:  ['nvenc_av1',  'amf_av1',  'qsv_av1'],
  }
  return map[codec]?.some(h => hw.includes(h)) ? 'HW' : null
}

function getCodecWarnLevel(codec: string): 'caution' | 'strong' | null {
  if (codec === 'av1'  && !hasAV1HW.value)  return 'strong'
  if (codec === 'h265' && !hasHEVCHW.value) return 'caution'
  return null
}

function selectVideoContainer(container: string) {
  if (!isContainerDisabled(container)) store.setFormat(container)
}

function selectVideoCodec(codec: string) {
  if (isCodecDisabled(codec)) return
  store.setSelectedCodec(codec)
  // If the current container is now incompatible, fall back to MP4
  const fmt = store.selectedFormat
  if (fmt && isContainerDisabled(fmt)) store.setFormat('mp4')
}

const videoConvertWarning = computed((): { level: 'caution' | 'strong'; message: string } | null => {
  const codec = store.selectedCodec
  if (codec === 'av1' && !hasAV1HW.value)
    return { level: 'strong',  message: 'AV1 のハードウェアエンコーダが見つかりません。CPU エンコードは非常に時間がかかります。' }
  if (codec === 'h265' && !hasHEVCHW.value)
    return { level: 'caution', message: 'H.265 のハードウェアエンコーダが見つかりません。長い動画は CPU エンコードで数分〜数十分かかる場合があります。' }
  return null
})

const showVideoConvertOptions = computed(() =>
  info.value?.type === 'video' && mode.value === 'convert',
)

// ── Video codec picker (compress / size-limit modes only) ─────────────────
const showCodecPicker = computed(() =>
  info.value?.type === 'video' && mode.value !== 'convert',
)

const videoCodecOptions = computed(() => [
  { value: null,   label: '自動',         note: 'HW優先で自動選択',                                          warn: false },
  { value: 'h264', label: 'H.264',        note: hasH264HW.value ? 'HW · 互換性最高' : 'CPU · 互換性最高',    warn: false },
  { value: 'h265', label: 'H.265 / HEVC', note: hasHEVCHW.value ? 'HW · 高圧縮'    : 'CPU · 高圧縮 (低速)', warn: !hasHEVCHW.value },
])

// ── Quality ────────────────────────────────────────────────────────────────

const QUALITY_OPTIONS: { value: QualityPreset; label: string; note: string }[] = [
  { value: 'high',     label: '高品質',  note: '劣化なし〜軽微' },
  { value: 'balanced', label: 'バランス', note: '劣化軽微' },
  { value: 'small',    label: '小さめ',  note: '劣化中程度' },
  { value: 'tiny',     label: '最小',    note: '劣化大きい' },
]

// ── Size limit ─────────────────────────────────────────────────────────────

const targetSizeMb  = ref(10)
const sizeUnit      = ref<'mb' | 'kb'>('mb')

function applyTargetSize() {
  const bytes = sizeUnit.value === 'mb'
    ? targetSizeMb.value * 1024 * 1024
    : targetSizeMb.value * 1024
  store.setTargetSize(Math.round(bytes))
}

function setDiscordPreset(tier: 'free' | 'nitro_basic' | 'nitro') {
  const mb = { free: 10, nitro_basic: 50, nitro: 500 }[tier]
  targetSizeMb.value = mb
  sizeUnit.value = 'mb'
  applyTargetSize()
}

// ── Lossless detection ────────────────────────────────────────────────────

/** Quality settings have no effect when the output format is lossless */
const isLosslessFormat = computed(() => {
  const fmt = store.selectedFormat
  return fmt === 'png' || fmt === 'flac'
})

// ── Execution ──────────────────────────────────────────────────────────────

const canExecute = computed(() => {
  if (mode.value === 'size-limit') return store.targetSizeBytes !== null
  if (mode.value === 'convert') {
    if (info.value?.type === 'video') {
      const fmt = store.selectedFormat
      return !!fmt && !!store.selectedCodec && !isContainerDisabled(fmt)
    }
    return store.selectedFormat !== null
  }
  return true
})

const execLabel = computed(() =>
  mode.value === 'convert' ? '変換を開始' : '圧縮を開始'
)

// ── File header helpers ────────────────────────────────────────────────────

const mediaBadge = computed(() => ({
  video: '動画', audio: '音声', image: '画像', archive: 'アーカイブ', unknown: 'ファイル',
})[info.value.type] ?? 'ファイル')

// Before/after comparison
const inputFormatLabel = computed(() => inputExt.value.toUpperCase() || '—')

const outputFormatLabel = computed(() => {
  const p = store.previewState
  if (p) return p.format.toUpperCase()
  const s = store.selectedFormat
  if (s) return s.toUpperCase()
  // In compress mode, output format = input format
  if (store.processMode === 'compress') return inputFormatLabel.value
  // For video convert, show container / codec
  if (showVideoConvertOptions.value && store.selectedFormat && store.selectedCodec) {
    const container = VIDEO_CONTAINERS.find(c => c.value === store.selectedFormat)
    const codec = VIDEO_CODECS.find(c => c.value === store.selectedCodec)
    if (container && codec) return `${container.label} / ${codec.label}`
  }
  return null
})
</script>

<template>
  <div class="recommend">
    <!-- Preview -->
    <FilePreview />

    <!-- File info -->
    <div class="file-header">
      <div class="file-meta">
        <span class="file-name">{{ store.fileName }}</span>
        <span class="badge">{{ mediaBadge }}</span>
        <span v-if="hasAlpha" class="badge badge-alpha">透過あり</span>
        <span v-if="info.codec" class="codec">{{ info.codec.toUpperCase() }}</span>
        <span v-if="info.width && info.height" class="dims">{{ info.width }}×{{ info.height }}</span>
      </div>

      <!-- Before → After comparison row -->
      <div class="conversion-row">
        <!-- Before -->
        <div class="conv-side">
          <span class="conv-fmt">{{ inputFormatLabel }}</span>
          <span class="conv-size">{{ formatBytes(info.size) }}</span>
        </div>

        <div class="conv-arrow">
          <span v-if="store.previewLoading" class="conv-spinner">
            <span class="mini-spinner" />
          </span>
          <span v-else class="arrow-icon">→</span>
        </div>

        <!-- After -->
        <div class="conv-side conv-after" :class="{ 'conv-pending': !store.previewState && !outputFormatLabel }">
          <template v-if="store.previewState">
            <span class="conv-fmt">{{ outputFormatLabel }}</span>
            <span class="conv-size accent">{{ formatBytes(store.previewState.outputSize) }}</span>
            <span class="conv-ratio">
              {{ reductionPercent(info.size, store.previewState.outputSize) }}
            </span>
          </template>
          <template v-else-if="outputFormatLabel">
            <span class="conv-fmt">{{ outputFormatLabel }}</span>
            <span class="conv-size muted">
              {{ info.type === 'image' ? '計算中…' : '—' }}
            </span>
          </template>
          <template v-else>
            <span class="conv-fmt muted">{{ mode === 'convert' ? '形式を選択' : '—' }}</span>
          </template>
        </div>
      </div>
    </div>

    <!-- ① 目的を選択 -->
    <div class="section">
      <span class="section-label">何をしたいですか？</span>
      <div class="mode-grid">
        <button
          v-for="m in MODES"
          :key="m.value"
          class="mode-card"
          :class="{ active: mode === m.value }"
          @click="selectMode(m.value)"
        >
          <span class="mode-icon">{{ m.icon }}</span>
          <span class="mode-title">{{ m.label }}</span>
          <p class="mode-desc">{{ m.desc }}</p>
        </button>
      </div>
    </div>

    <!-- ② 形式 / コーデック（モード別） -->

    <!-- 動画 convert: コンテナとコーデック別々選択 -->
    <template v-if="showVideoConvertOptions">
      <div class="section">
        <span class="section-label">コンテナ</span>
        <div class="option-btns">
          <button
            v-for="c in VIDEO_CONTAINERS"
            :key="c.value"
            class="option-btn"
            :class="{
              active:        store.selectedFormat === c.value,
              'opt-disabled': isContainerDisabled(c.value),
            }"
            :title="isContainerDisabled(c.value) ? '選択中のコーデックと組み合わせできません' : ''"
            @click="selectVideoContainer(c.value)"
          >
            {{ c.label }}
            <span class="option-note">{{ c.note }}</span>
          </button>
        </div>
      </div>
      <div class="section">
        <span class="section-label">コーデック</span>
        <div class="option-btns">
          <button
            v-for="cd in VIDEO_CODECS"
            :key="cd.value"
            class="option-btn"
            :class="{
              active:          store.selectedCodec === cd.value,
              'opt-disabled':  isCodecDisabled(cd.value),
              'warn-caution':  getCodecWarnLevel(cd.value) === 'caution',
              'warn-strong':   getCodecWarnLevel(cd.value) === 'strong',
            }"
            :title="isCodecDisabled(cd.value) ? 'HWエンコーダなし。設定から CPU AV1 を許可してください' : ''"
            @click="selectVideoCodec(cd.value)"
          >
            {{ cd.label }}
            <span class="option-note">{{ cd.note }}</span>
            <span v-if="getCodecHwBadge(cd.value)" class="hw-badge hw-ok">HW</span>
            <span v-else-if="getCodecWarnLevel(cd.value) === 'strong'"  class="hw-badge hw-none">HW なし</span>
            <span v-else-if="getCodecWarnLevel(cd.value) === 'caution'" class="hw-badge hw-slow">低速</span>
          </button>
        </div>
        <p v-if="!store.selectedCodec" class="config-hint">コーデックを選択してください</p>
        <p v-else-if="videoConvertWarning" class="warn-hint" :class="videoConvertWarning.level">
          {{ videoConvertWarning.level === 'strong' ? '⚠️' : '⚠' }}
          {{ videoConvertWarning.message }}
        </p>
      </div>
    </template>

    <!-- 画像 / 音声 convert: フォーマット選択 -->
    <div v-else-if="mode === 'convert'" class="section">
      <span class="section-label">出力形式</span>
      <div class="option-btns">
        <button
          v-for="f in formatOptions"
          :key="f.value"
          class="option-btn"
          :class="{ active: store.selectedFormat === f.value, warn: f.warn }"
          @click="store.setFormat(f.value)"
        >
          {{ f.label }}
          <span v-if="f.note" class="option-note" :class="{ 'note-warn': f.warn }">{{ f.note }}</span>
        </button>
      </div>
      <p v-if="!store.selectedFormat" class="config-hint">形式を選択してください</p>
      <p v-else-if="hasAlpha && !ALPHA_CAPABLE.includes(store.selectedFormat)" class="warn-hint">
        ⚠ このファイルは透過情報を含みます。{{ store.selectedFormat.toUpperCase() }} では透過が失われます。
      </p>
    </div>

    <!-- 動画 compress/size-limit: コーデック選択のみ（形式は入力と同じ） -->
    <div v-if="showCodecPicker" class="section">
      <span class="section-label">コーデック</span>
      <div class="option-btns">
        <button
          v-for="c in videoCodecOptions"
          :key="String(c.value)"
          class="option-btn"
          :class="{
            active: store.selectedCodec === c.value,
            'warn-caution': c.warn,
          }"
          @click="store.setSelectedCodec(c.value)"
        >
          {{ c.label }}
          <span class="option-note">{{ c.note }}</span>
          <span v-if="c.warn" class="hw-badge hw-slow">低速</span>
        </button>
      </div>
      <!-- H.265 caution when selected without HW -->
      <p v-if="store.selectedCodec === 'libx265' && !hasHEVCHW" class="warn-hint caution">
        ⚠ H.265 のハードウェアエンコーダが見つかりません。長い動画は CPU エンコードで数分〜数十分かかる場合があります。
      </p>
    </div>

    <!-- サイズ指定 -->
    <div v-if="mode === 'size-limit'" class="section">
      <span class="section-label">目標サイズ</span>
      <div class="size-row">
        <input
          type="number"
          class="size-input"
          v-model.number="targetSizeMb"
          min="0.1"
          step="0.5"
          @change="applyTargetSize"
        />
        <select class="size-unit" v-model="sizeUnit" @change="applyTargetSize">
          <option value="mb">MB</option>
          <option value="kb">KB</option>
        </select>
      </div>
      <div class="size-presets">
        <button class="size-preset-btn" @click="setDiscordPreset('free')">
          Discord Free
          <span class="option-note">10 MB</span>
        </button>
        <button class="size-preset-btn" @click="setDiscordPreset('nitro_basic')">
          Nitro Basic
          <span class="option-note">50 MB</span>
        </button>
        <button class="size-preset-btn" @click="setDiscordPreset('nitro')">
          Nitro
          <span class="option-note">500 MB</span>
        </button>
      </div>
    </div>

    <!-- ③ 品質（常に表示、ロスレス形式のときはグレーアウト） -->
    <div class="section" :class="{ 'section-lossless': isLosslessFormat }">
      <div class="section-label-row">
        <span class="section-label">品質・劣化具合</span>
        <span v-if="isLosslessFormat" class="lossless-badge">
          ロスレス形式 — 品質設定は無効
        </span>
      </div>
      <div class="quality-grid" :inert="isLosslessFormat || undefined">
        <button
          v-for="q in QUALITY_OPTIONS"
          :key="q.value"
          class="quality-btn"
          :class="{ active: store.qualityPreset === q.value }"
          @click="store.setQualityPreset(q.value)"
        >
          <span class="quality-label">{{ q.label }}</span>
          <span class="quality-note">{{ q.note }}</span>
        </button>
      </div>
    </div>

    <!-- Actions -->
    <div class="actions">
      <button class="btn-secondary" @click="store.reset()">← 戻る</button>
      <button
        class="btn-primary"
        :disabled="!canExecute"
        @click="store.execute()"
      >
        {{ execLabel }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.recommend {
  width: 100%;
  max-width: 680px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

/* File header */
.file-header {
  background: var(--surface);
  border-radius: 12px;
  padding: 14px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.file-meta { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }

.file-name {
  font-weight: 600;
  font-size: 0.95rem;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 360px;
}

.badge {
  background: var(--accent-tint);
  color: var(--accent);
  font-size: 0.72rem;
  font-weight: 600;
  padding: 2px 7px;
  border-radius: 99px;
  white-space: nowrap;
}

.badge-alpha {
  background: repeating-conic-gradient(#0000 0% 25%, rgba(150,150,150,0.25) 0% 50%) 50% / 8px 8px;
  color: var(--text);
}

.codec, .dims {
  font-family: monospace;
  font-size: 0.75rem;
  background: var(--bg);
  padding: 1px 6px;
  border-radius: 4px;
  color: var(--muted);
}

/* Before → After comparison */
.conversion-row {
  display: flex;
  align-items: center;
  gap: 12px;
  background: var(--bg);
  border-radius: 8px;
  padding: 10px 14px;
}

.conv-side {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1;
}

.conv-after { justify-content: flex-start; }
.conv-pending { opacity: 0.5; }

.conv-fmt {
  font-family: monospace;
  font-size: 0.8rem;
  font-weight: 700;
  background: var(--surface);
  padding: 2px 7px;
  border-radius: 5px;
  color: var(--text);
  white-space: nowrap;
}
.conv-fmt.muted { color: var(--muted); }

.conv-size {
  font-size: 0.9rem;
  font-weight: 700;
  color: var(--text);
  white-space: nowrap;
}
.conv-size.accent { color: var(--accent); }
.conv-size.muted  { color: var(--muted); font-weight: 400; font-style: italic; }

.conv-ratio {
  font-size: 0.8rem;
  font-weight: 700;
  color: #22c55e;
  white-space: nowrap;
}

.conv-arrow {
  color: var(--muted);
  font-size: 1.1rem;
  flex-shrink: 0;
  display: flex;
  align-items: center;
}

.arrow-icon { color: var(--border-hover); }

.conv-spinner { display: flex; align-items: center; }

.mini-spinner {
  display: inline-block;
  width: 14px; height: 14px;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

/* Section wrapper */
.section {
  background: var(--surface);
  border-radius: 12px;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.section-label {
  font-size: 0.78rem;
  font-weight: 600;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

/* Mode grid */
.mode-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 10px;
}

.mode-card {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  padding: 14px 12px;
  background: var(--bg);
  border: 2px solid transparent;
  border-radius: 10px;
  cursor: pointer;
  text-align: left;
  transition: border-color 0.12s, background 0.12s;
}

.mode-card:hover { border-color: var(--border-hover); }
.mode-card.active { border-color: var(--accent); background: var(--accent-tint); }

.mode-icon  { font-size: 1.4rem; }
.mode-title { font-size: 0.88rem; font-weight: 700; color: var(--text); }
.mode-desc  { font-size: 0.72rem; color: var(--muted); margin: 0; line-height: 1.4; }

/* Option buttons (format / codec) */
.option-btns {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.option-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 8px 14px;
  border: 2px solid var(--border);
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  font-size: 0.85rem;
  font-weight: 500;
  transition: border-color 0.12s, color 0.12s;
  gap: 2px;
}

.option-btn:hover              { border-color: var(--border-hover); }
.option-btn.active             { border-color: var(--accent); color: var(--accent); }
.option-btn.opt-disabled       { opacity: 0.35; cursor: not-allowed; }
.option-btn.opt-disabled:hover { border-color: var(--border); }
.option-btn.warn.active        { border-color: #f59e0b; color: #f59e0b; }
.option-btn.warn-caution       { border-color: rgba(245,158,11,0.4); }
.option-btn.warn-caution.active{ border-color: #f59e0b; color: #f59e0b; }
.option-btn.warn-strong        { border-color: rgba(239,68,68,0.35); }
.option-btn.warn-strong.active { border-color: #ef4444; color: #ef4444; }

.hw-badge {
  font-size: 0.62rem;
  font-weight: 700;
  padding: 1px 5px;
  border-radius: 3px;
  margin-top: 2px;
}
.hw-none { background: rgba(239,68,68,0.15); color: #ef4444; }
.hw-slow { background: rgba(245,158,11,0.15); color: #f59e0b; }

.option-note { font-size: 0.68rem; color: var(--muted); font-weight: 400; }
.note-warn   { color: #f59e0b; }

.config-hint { font-size: 0.78rem; color: var(--muted); margin: 0; }
.warn-hint          { font-size: 0.78rem; color: #f59e0b; margin: 0; line-height: 1.5; }
.warn-hint.caution  { color: #f59e0b; }
.warn-hint.strong   { color: #ef4444; }

/* Size limit controls */
.size-row {
  display: flex;
  gap: 8px;
  align-items: center;
}

.size-input {
  width: 90px;
  padding: 8px 10px;
  border: 2px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
  color: var(--text);
  font-size: 0.95rem;
  font-weight: 600;
  text-align: right;
}

.size-unit {
  padding: 8px 10px;
  border: 2px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
  color: var(--text);
  font-size: 0.85rem;
  cursor: pointer;
}

.size-presets {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.size-preset-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 6px 14px;
  border: 2px solid var(--border);
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  cursor: pointer;
  font-size: 0.8rem;
  font-weight: 600;
  transition: border-color 0.12s;
}

.size-preset-btn:hover { border-color: var(--accent); color: var(--accent); }

/* Lossless */
.section-lossless .quality-grid {
  opacity: 0.35;
  pointer-events: none;
}

.section-label-row {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
}

.lossless-badge {
  font-size: 0.72rem;
  color: var(--muted);
  background: var(--bg);
  padding: 2px 8px;
  border-radius: 99px;
  border: 1px solid var(--border);
}

/* Quality grid */
.quality-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 8px;
}

.quality-btn {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  padding: 10px 6px;
  border: 2px solid var(--border);
  border-radius: 8px;
  background: transparent;
  cursor: pointer;
  transition: border-color 0.12s;
}

.quality-btn:hover  { border-color: var(--border-hover); }
.quality-btn.active { border-color: var(--accent); background: var(--accent-tint); }

.quality-label { font-size: 0.85rem; font-weight: 700; color: var(--text); }
.quality-note  { font-size: 0.68rem; color: var(--muted); text-align: center; }

/* Actions */
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 12px;
  padding-top: 4px;
}

.btn-primary {
  padding: 12px 32px;
  background: var(--accent);
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.95rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s;
}

.btn-primary:hover:not(:disabled) { opacity: 0.85; }
.btn-primary:disabled { opacity: 0.4; cursor: not-allowed; }

.btn-secondary {
  padding: 12px 20px;
  background: transparent;
  color: var(--muted);
  border: 2px solid var(--border);
  border-radius: 8px;
  font-size: 0.9rem;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}

.btn-secondary:hover { color: var(--text); border-color: var(--border-hover); }

@keyframes spin { to { transform: rotate(360deg); } }
</style>
