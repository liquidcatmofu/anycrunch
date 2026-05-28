import { isTauri } from '@tauri-apps/api/core'
import type { Ref } from 'vue'

export async function setupFileDrop(
  el: HTMLElement,
  onFile: (file: File | string) => void,
  isDragOver: Ref<boolean>
): Promise<() => void> {
  const onDragOver = (e: DragEvent) => { e.preventDefault(); isDragOver.value = true }
  const onDragLeave = (e: DragEvent) => {
    if (!el.contains(e.relatedTarget as Node)) isDragOver.value = false
  }

  el.addEventListener('dragover', onDragOver)
  el.addEventListener('dragleave', onDragLeave)

  if (isTauri()) {
    // Suppress HTML drop; Tauri native event provides the path
    const suppressDrop = (e: DragEvent) => { e.preventDefault(); isDragOver.value = false }
    el.addEventListener('drop', suppressDrop)

    const { getCurrentWebview } = await import('@tauri-apps/api/webview')
    const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      const { type } = event.payload
      if (type === 'enter' || type === 'over') {
        isDragOver.value = true
      } else if (type === 'leave') {
        isDragOver.value = false
      } else if (type === 'drop' && 'paths' in event.payload && event.payload.paths.length > 0) {
        isDragOver.value = false
        onFile(event.payload.paths[0])
      }
    })

    return () => {
      el.removeEventListener('dragover', onDragOver)
      el.removeEventListener('dragleave', onDragLeave)
      el.removeEventListener('drop', suppressDrop)
      unlisten()
    }
  } else {
    const onDrop = (e: DragEvent) => {
      e.preventDefault()
      isDragOver.value = false
      const file = e.dataTransfer?.files[0]
      if (file) onFile(file)
    }
    el.addEventListener('drop', onDrop)
    return () => {
      el.removeEventListener('dragover', onDragOver)
      el.removeEventListener('dragleave', onDragLeave)
      el.removeEventListener('drop', onDrop)
    }
  }
}

export async function openFilePicker(onFile: (file: File | string) => void) {
  if (isTauri()) {
    const { open } = await import('@tauri-apps/plugin-dialog')
    const result = await open({ multiple: false })
    if (typeof result === 'string') onFile(result)
  } else {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = 'image/*,video/*,audio/*,.zip,.7z,.tar,.gz,.bz2,.rar,.xz,.zst'
    input.onchange = () => {
      const file = input.files?.[0]
      if (file) onFile(file)
    }
    input.click()
  }
}
