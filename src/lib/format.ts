export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`
}

export function formatDuration(seconds: number): string {
  const m = Math.floor(seconds / 60)
  const s = Math.floor(seconds % 60)
  return m > 0 ? `${m}分${s}秒` : `${s}秒`
}

export function formatEta(seconds: number): string {
  if (seconds < 5) return 'まもなく完了'
  return `残り約${formatDuration(seconds)}`
}

export function reductionPercent(original: number, output: number): string {
  if (original === 0) return '0%'
  const pct = ((original - output) / original) * 100
  return `${pct > 0 ? '-' : '+'}${Math.abs(pct).toFixed(1)}%`
}
