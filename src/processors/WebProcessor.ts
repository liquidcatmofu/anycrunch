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

export class WebProcessor implements MediaProcessor {
  readonly platform = 'web' as const

  async analyze(_file: File | string): Promise<MediaInfo> {
    throw new Error('Not implemented')
  }

  async recommend(
    _info: MediaInfo,
    _useCase: UseCase,
    _settings: AppSettings
  ): Promise<RecommendResult> {
    throw new Error('Not implemented')
  }

  async detectHwAccel(): Promise<string[]> {
    return []
  }

  async process(
    _input: File | string,
    _options: ProcessOptions,
    _onProgress: (progress: ProcessProgress) => void
  ): Promise<ProcessResult> {
    throw new Error('Not implemented')
  }

  async cancel(_fileId: string): Promise<void> {
    throw new Error('Not implemented')
  }
}
