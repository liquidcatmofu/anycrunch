import { invoke, Channel } from '@tauri-apps/api/core'
import type {
  MediaProcessor,
  MediaInfo,
  UseCase,
  AppSettings,
  RecommendResult,
  ProcessOptions,
  ProcessProgress,
  ProcessResult,
} from '../types'
import { selectCodec } from '../lib/codec'
import { buildEstimate } from '../lib/estimate'

export class TauriProcessor implements MediaProcessor {
  readonly platform = 'desktop' as const

  async analyze(file: File | string): Promise<MediaInfo> {
    const path = typeof file === 'string' ? file : (file as any).path ?? file.name
    return invoke<MediaInfo>('analyze_file', { path })
  }

  async recommend(
    info: MediaInfo,
    useCase: UseCase,
    settings: AppSettings
  ): Promise<RecommendResult> {
    const hwAccel = await this.detectHwAccel()
    const codecRec = selectCodec(info, hwAccel, settings)
    const estimate = buildEstimate(info, useCase)

    return {
      info,
      estimate,
      recommended: useCase,
      reasoning: codecRec.message ?? `${codecRec.codec.toUpperCase()} (${codecRec.encoder})`,
    }
  }

  async detectHwAccel(): Promise<string[]> {
    return invoke<string[]>('detect_hw_accel')
  }

  async process(
    input: File | string,
    options: ProcessOptions,
    onProgress: (progress: ProcessProgress) => void
  ): Promise<ProcessResult> {
    const path = typeof input === 'string' ? input : (input as any).path ?? input.name
    const fileId = `${Date.now()}-${Math.random().toString(36).slice(2)}`

    const channel = new Channel<ProcessProgress>()
    channel.onmessage = onProgress

    return invoke<ProcessResult>('process_file', {
      fileId,
      input: path,
      options,
      onProgress: channel,
    })
  }

  async cancel(fileId: string): Promise<void> {
    return invoke('cancel_process', { fileId })
  }
}
