import { isTauri } from '@tauri-apps/api/core'
import type { MediaProcessor } from '../types'
import { TauriProcessor } from '../processors/TauriProcessor'
import { WebProcessor } from '../processors/WebProcessor'

export function useProcessor(): MediaProcessor {
  if (isTauri()) return new TauriProcessor()
  return new WebProcessor()
}
