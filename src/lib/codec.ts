import type { MediaInfo, AppSettings } from '../types'

type WarningLevel = 'none' | 'caution' | 'strong'

export interface CodecRecommendation {
  codec: string
  encoder: string
  warning: WarningLevel
  message?: string
  isCpuAV1?: boolean
}

function detectAV1Encoder(hwAccel: string[]): string {
  if (hwAccel.includes('nvenc_av1')) return 'av1_nvenc'
  if (hwAccel.includes('amf_av1'))   return 'av1_amf'
  if (hwAccel.includes('qsv_av1'))   return 'av1_qsv'
  return 'libaom-av1'
}

function detectH265Encoder(hwAccel: string[]): string {
  if (hwAccel.includes('nvenc_hevc'))         return 'hevc_nvenc'
  if (hwAccel.includes('amf_hevc'))           return 'hevc_amf'
  if (hwAccel.includes('qsv_hevc'))           return 'hevc_qsv'
  if (hwAccel.includes('videotoolbox_hevc'))  return 'hevc_videotoolbox'
  return 'libx265'
}

function estimateCpuMinutes(info: MediaInfo): number {
  const dur = info.duration ?? 0
  return Math.round(dur / 60 * 3) // rough: 3x realtime for CPU x265
}

export function selectCodec(
  info: MediaInfo,
  hwAccel: string[],
  settings: AppSettings
): CodecRecommendation {
  const hasAV1Hw  = hwAccel.some(h => ['nvenc_av1', 'amf_av1', 'qsv_av1'].includes(h))
  const hasH265Hw = hwAccel.some(h => ['nvenc_hevc', 'amf_hevc', 'qsv_hevc', 'videotoolbox_hevc'].includes(h))

  if (hasAV1Hw) {
    return { codec: 'av1', encoder: detectAV1Encoder(hwAccel), warning: 'none' }
  }
  if (hasH265Hw) {
    return { codec: 'h265', encoder: detectH265Encoder(hwAccel), warning: 'none' }
  }

  // No HW — long content
  if (info.duration && info.duration > 300) {
    return {
      codec: 'h265',
      encoder: 'libx265',
      warning: 'strong',
      message: `CPUエンコードで推定${estimateCpuMinutes(info)}分かかります。H.264への変更を推奨します`,
    }
  }

  // CPU AV1 opt-in
  if (settings.allowCpuAV1) {
    return {
      codec: 'av1',
      encoder: 'libaom-av1',
      warning: 'strong',
      message: 'CPU AV1エンコードは非常に時間がかかります',
      isCpuAV1: true,
    }
  }

  return { codec: 'h264', encoder: 'libx264', warning: 'none' }
}
