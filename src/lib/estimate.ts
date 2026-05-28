import type { MediaInfo, UseCase, CompressionEstimate } from '../types'

// ── Space efficiency model ──────────────────────────────────────────────────
// "space efficiency" = how few bytes are needed for equivalent perceived quality.
// Higher = more efficient (smaller files). Lossless formats score low because
// they store everything. This is NOT the same as "modernness".

function spaceEfficiency(info: MediaInfo): number {
  const codec = (info.codec ?? '').toLowerCase()
  if (info.type === 'image') {
    if (codec.includes('av1') || info.mimeType === 'image/avif') return 95
    if (codec.includes('webp')) return 85
    if (codec.includes('mjpeg') || codec.includes('jpeg')) return 70
    if (codec.includes('hevc') || info.mimeType.includes('heic')) return 90
    if (codec.includes('png')) return 35          // lossless
    if (codec.includes('bmp') || codec.includes('tiff')) return 8
    return 60
  }
  if (info.type === 'video') {
    if (codec.includes('av1')) return 95
    if (codec.includes('hevc') || codec.includes('h265')) return 80
    if (codec.includes('vp9')) return 78
    if (codec.includes('h264') || codec.includes('avc')) return 65
    if (codec.includes('vp8')) return 55
    if (codec.includes('mpeg4') || codec.includes('mpeg2')) return 45
    return 60
  }
  if (info.type === 'audio') {
    if (codec.includes('opus')) return 95
    if (codec.includes('aac')) return 75
    if (codec.includes('vorbis')) return 72
    if (codec.includes('mp3')) return 65
    if (codec.includes('flac') || codec.includes('alac')) return 35  // lossless
    if (codec.includes('pcm') || codec.includes('wav')) return 8     // uncompressed
    return 60
  }
  return 70
}

// Representative target codec efficiency per preset + media type
function targetEfficiency(useCaseType: string, mediaType: string): number {
  if (mediaType === 'image') {
    // all presets default to JPEG output (always available)
    return 70
  }
  if (mediaType === 'video') {
    switch (useCaseType) {
      case 'storage': return 82   // HEVC/AV1
      case 'web':     return 65   // H.264 for compatibility
      case 'discord': return 65   // H.264, aggressive bitrate
      default:        return 70
    }
  }
  if (mediaType === 'audio') {
    switch (useCaseType) {
      case 'storage': return 95   // Opus
      case 'web':     return 75   // AAC
      case 'discord': return 95   // Opus
      default:        return 80
    }
  }
  return 70
}

// How much additional compression each preset applies beyond the codec swap.
// Lower = smaller output but more quality loss.
function qualityRetention(useCaseType: string): number {
  switch (useCaseType) {
    case 'storage': return 0.95
    case 'web':     return 0.82
    case 'discord': return 0.55
    default:        return 0.85
  }
}

// Compute the estimated output/input size ratio for a given preset.
function estimateRatio(info: MediaInfo, useCaseType: string): number {
  // Archives: handled separately, rough fixed guess
  if (info.type === 'archive' || info.type === 'unknown') return 0.7

  const srcEff = spaceEfficiency(info)
  const tgtEff = targetEfficiency(useCaseType, info.type)
  const retention = qualityRetention(useCaseType)

  // size ∝ 1 / efficiency, so converting src→tgt scales by srcEff/tgtEff,
  // then quality retention compresses further.
  const ratio = (srcEff / tgtEff) * retention
  // clamp: never below 5%, allow >1 (grows) up to 1.15 to signal "don't bother"
  return Math.min(Math.max(ratio, 0.05), 1.15)
}

function qualityLossFor(useCaseType: string, ratio: number, info: MediaInfo): CompressionEstimate['presets'][number]['qualityLoss'] {
  // Barely changes → effectively no loss
  if (ratio >= 0.97) return 'none'

  const lossySource = !['png', 'flac', 'alac', 'pcm', 'wav', 'bmp', 'tiff']
    .some(c => (info.codec ?? '').toLowerCase().includes(c))

  switch (useCaseType) {
    case 'storage': return lossySource ? 'minimal' : 'none'
    case 'web':     return 'minimal'
    case 'discord': return ratio < 0.4 ? 'significant' : 'moderate'
    default:        return 'minimal'
  }
}

function estimateTime(info: MediaInfo): number {
  if (info.type === 'image') return 1
  if (info.type === 'audio') return Math.max(1, (info.duration ?? 0) * 0.05)
  // video: scale by resolution as a rough complexity proxy
  const pixels = (info.width ?? 1280) * (info.height ?? 720)
  const resFactor = pixels / (1280 * 720)
  return Math.max(2, (info.duration ?? 0) * 0.3 * resFactor)
}

export function buildEstimate(info: MediaInfo, _useCase: UseCase): CompressionEstimate {
  const presetTypes: { useCase: UseCase; type: string }[] = [
    { useCase: { type: 'storage' },               type: 'storage' },
    { useCase: { type: 'web' },                    type: 'web' },
    { useCase: { type: 'discord', tier: 'free' },  type: 'discord' },
  ]

  const presets = presetTypes.map(({ useCase, type }) => {
    const ratio = estimateRatio(info, type)
    return {
      useCase,
      estimatedSize: Math.round(info.size * ratio),
      estimatedSizeRatio: ratio,
      estimatedTime: estimateTime(info),
      qualityLoss: qualityLossFor(type, ratio, info),
    }
  })

  // Redundancy = how much the BEST preset can save (most meaningful single number)
  const bestRatio = Math.min(...presets.map(p => p.estimatedSizeRatio))
  const redundancy = Math.round(Math.min(Math.max((1 - bestRatio) * 100, 0), 100))
  const efficiency = 100 - redundancy

  return { currentScore: { efficiency, redundancy }, presets }
}
