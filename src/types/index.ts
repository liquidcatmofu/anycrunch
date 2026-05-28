export interface MediaInfo {
  path: string
  type: 'image' | 'video' | 'audio' | 'archive' | 'unknown'
  size: number
  mimeType: string
  width?: number
  height?: number
  duration?: number
  bitrate?: number
  codec?: string
  fps?: number
  hasAlpha?: boolean
}

export type DiscordTier = 'free' | 'nitro_basic' | 'nitro'

export const DISCORD_LIMITS: Record<DiscordTier, number> = {
  free:        10  * 1024 * 1024,
  nitro_basic: 50  * 1024 * 1024,
  nitro:       500 * 1024 * 1024,
}

export type CompatTarget = 'universal' | 'windows' | 'ios'

export type UseCase =
  | { type: 'discord';         tier: DiscordTier }
  | { type: 'discord_animate'; tier: DiscordTier }
  | { type: 'web' }
  | { type: 'storage' }
  | { type: 'archive'; priority: 'speed' | 'balanced' | 'size' }
  | { type: 'compat'; target: CompatTarget }
  | { type: 'extract' }
  | { type: 'custom' }

export interface TransformOptions {
  resize?: {
    width?: number
    height?: number
    mode: 'fit' | 'fill' | 'stretch' | 'crop'
    anchor?: 'center' | 'top' | 'bottom' | 'left' | 'right'
  }
  aspectRatio?: {
    ratio: '16:9' | '4:3' | '1:1' | '9:16' | 'custom'
    customX?: number
    customY?: number
  }
}

export interface ProcessOptions {
  useCase: UseCase
  target?: {
    maxSizeBytes?: number
    maxWidth?: number
    maxHeight?: number
    quality?: number
  }
  codec?: string
  format?: string
  hwAccel?: boolean
  transform?: TransformOptions
  description: string
}

export interface CompressionEstimate {
  currentScore: {
    efficiency: number
    redundancy: number
  }
  presets: {
    useCase: UseCase
    estimatedSize: number
    estimatedSizeRatio: number
    estimatedTime: number
    qualityLoss: 'none' | 'minimal' | 'moderate' | 'significant'
  }[]
}

export interface RecommendResult {
  info: MediaInfo
  estimate: CompressionEstimate
  recommended: UseCase
  reasoning: string
}

export interface ProcessProgress {
  fileId: string
  percent: number
  eta?: number
  currentStep: string
}

export interface ProcessResult {
  success: boolean
  outputPath: string
  originalSize: number
  outputSize: number
  duration: number
  error?: string
}

export interface AppSettings {
  ffmpegPath?: string
  avifencPath?: string
  avifdecPath?: string
  allowCpuAV1: boolean
}

export interface MediaProcessor {
  readonly platform: 'web' | 'desktop'
  analyze(file: File | string): Promise<MediaInfo>
  recommend(info: MediaInfo, useCase: UseCase, settings: AppSettings): Promise<RecommendResult>
  detectHwAccel(): Promise<string[]>
  process(
    input: File | string,
    options: ProcessOptions,
    onProgress: (progress: ProcessProgress) => void
  ): Promise<ProcessResult>
  cancel(fileId: string): Promise<void>
}
