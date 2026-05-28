<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, watch } from 'vue'
import { isTauri } from '@tauri-apps/api/core'
import { useProcessorStore } from '../stores/processor'
import { formatBytes } from '../lib/format'

const store = useProcessorStore()
const info = computed(() => store.mediaInfo!)
const originalSrc = ref<string | null>(null)
const previewSrc  = ref<string | null>(null)
let objectUrl: string | null = null
let previewObjectUrl: string | null = null

const showOriginal = ref(true)

onMounted(async () => {
  const file = store.file
  if (!file) return
  if (typeof file === 'string') {
    if (isTauri()) {
      const { convertFileSrc } = await import('@tauri-apps/api/core')
      originalSrc.value = convertFileSrc(file)
    }
  } else {
    objectUrl = URL.createObjectURL(file)
    originalSrc.value = objectUrl
  }
})

onUnmounted(() => {
  if (objectUrl) URL.revokeObjectURL(objectUrl)
  if (previewObjectUrl) URL.revokeObjectURL(previewObjectUrl)
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('mouseup', onMouseUp)
})

// Single watcher: handles both null→state and state→null transitions atomically
watch(() => store.previewState, async (state) => {
  if (previewObjectUrl) { URL.revokeObjectURL(previewObjectUrl); previewObjectUrl = null }
  if (!state) {
    previewSrc.value = null
    showOriginal.value = true
    return
  }
  if (isTauri()) {
    const { convertFileSrc } = await import('@tauri-apps/api/core')
    previewSrc.value = convertFileSrc(state.path)
  }
  showOriginal.value = false
}, { immediate: true })

// Reset zoom when switching original/processed inside the modal
watch(showOriginal, () => { if (showZoom.value) resetZoom() })

const hasPreview = computed(() => !!store.previewState)
const displaySrc = computed(() => showOriginal.value ? originalSrc.value : previewSrc.value)

const isImage = computed(() => info.value?.type === 'image')
const isVideo = computed(() => info.value?.type === 'video')
const isAudio = computed(() => info.value?.type === 'audio')

const imgError = ref(false)

const reductionBadge = computed(() => {
  const s = store.previewState
  if (!s || !info.value) return null
  const ratio = 1 - s.outputSize / info.value.size
  if (ratio <= 0) return null
  return `-${Math.round(ratio * 100)}%`
})

// ── Zoom modal ─────────────────────────────────────────────────────────────

const showZoom    = ref(false)
const zoomLevel   = ref(1)
const zoomImgEl   = ref<HTMLImageElement | null>(null)
const zoomWrapEl  = ref<HTMLElement | null>(null)
const imgNatW     = ref(0)
const imgNatH     = ref(0)

let isDragging  = false
let dragStart   = { x: 0, y: 0 }
let scrollStart = { x: 0, y: 0 }

function openZoom() {
  if (!isImage.value || !displaySrc.value) return
  zoomLevel.value = 1
  showZoom.value = true
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('mouseup', onMouseUp)
}

function closeZoom() {
  showZoom.value = false
  isDragging = false
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('mouseup', onMouseUp)
}

function resetZoom() { zoomLevel.value = 1 }

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') closeZoom()
}

function onImgLoad() {
  if (zoomImgEl.value) {
    imgNatW.value = zoomImgEl.value.naturalWidth
    imgNatH.value = zoomImgEl.value.naturalHeight
  }
}

// Scroll wheel → zoom in/out centred on the mouse position
function onWheelZoom(e: WheelEvent) {
  e.preventDefault()
  const wrap = zoomWrapEl.value
  if (!wrap) return

  const factor = e.deltaY > 0 ? 0.85 : 1.18
  const oldZoom = zoomLevel.value
  const newZoom = Math.max(0.2, Math.min(12, oldZoom * factor))

  // Keep the point under the mouse fixed while zooming
  if (imgNatW.value && imgNatH.value) {
    const rect = wrap.getBoundingClientRect()
    const mouseX = e.clientX - rect.left + wrap.scrollLeft
    const mouseY = e.clientY - rect.top  + wrap.scrollTop
    const scale  = newZoom / oldZoom
    wrap.scrollLeft = mouseX * scale - (e.clientX - rect.left)
    wrap.scrollTop  = mouseY * scale - (e.clientY - rect.top)
  }

  zoomLevel.value = newZoom
}

// Double-click → toggle 2× / fit
function onDblClick() {
  zoomLevel.value = zoomLevel.value > 1.05 ? 1 : 2
}

// Drag to pan (when zoomed in)
function onMouseDown(e: MouseEvent) {
  if (zoomLevel.value <= 1 || !zoomWrapEl.value) return
  isDragging  = true
  dragStart   = { x: e.clientX, y: e.clientY }
  scrollStart = { x: zoomWrapEl.value.scrollLeft, y: zoomWrapEl.value.scrollTop }
  e.preventDefault()
}

function onMouseMove(e: MouseEvent) {
  if (!isDragging || !zoomWrapEl.value) return
  zoomWrapEl.value.scrollLeft = scrollStart.x - (e.clientX - dragStart.x)
  zoomWrapEl.value.scrollTop  = scrollStart.y - (e.clientY - dragStart.y)
}

function onMouseUp() { isDragging = false }

// Image style inside the modal
const imgZoomStyle = computed(() => {
  const z = zoomLevel.value
  if (!imgNatW.value || !imgNatH.value || Math.abs(z - 1) < 0.01) return {}
  return {
    width:    `${imgNatW.value * z}px`,
    height:   `${imgNatH.value * z}px`,
    maxWidth:  'none',
    maxHeight: 'none',
  }
})

const zoomCursor = computed(() => {
  if (zoomLevel.value <= 1) return 'zoom-in'
  return isDragging ? 'grabbing' : 'grab'
})

const zoomPct = computed(() => `${Math.round(zoomLevel.value * 100)}%`)
</script>

<template>
  <div class="preview-wrap">
    <!-- Toggle bar -->
    <div v-if="isImage && (hasPreview || store.previewLoading)" class="preview-toggle-bar">
      <button class="toggle-btn" :class="{ active: showOriginal }" @click="showOriginal = true">
        元画像
        <span class="toggle-size">{{ formatBytes(info.size) }}</span>
      </button>
      <button class="toggle-btn" :class="{ active: !showOriginal }" :disabled="!hasPreview" @click="showOriginal = false">
        処理後
        <span v-if="store.previewState" class="toggle-size">
          {{ formatBytes(store.previewState.outputSize) }}
          <span v-if="reductionBadge" class="toggle-ratio">{{ reductionBadge }}</span>
        </span>
        <span v-else-if="store.previewLoading" class="toggle-size muted">計算中…</span>
      </button>
    </div>

    <!-- Spinner overlay -->
    <div v-if="store.previewLoading && showOriginal" class="spinner-overlay">
      <div class="spinner" />
    </div>

    <!-- Image (zoomable) -->
    <template v-if="isImage && displaySrc && !imgError">
      <div class="img-container zoomable" @click="openZoom">
        <img :src="displaySrc" class="preview-img" alt="preview" @error="imgError = true" />
        <div class="zoom-hint">🔍 クリックで拡大</div>
      </div>
    </template>
    <template v-else-if="isImage">
      <div class="preview-placeholder"><span class="placeholder-icon">🖼</span></div>
    </template>

    <!-- Video -->
    <template v-else-if="isVideo && originalSrc">
      <video :src="originalSrc" class="preview-video" controls preload="metadata" muted />
    </template>

    <!-- Audio -->
    <template v-else-if="isAudio && originalSrc">
      <div class="preview-audio">
        <span class="placeholder-icon">🎵</span>
        <audio :src="originalSrc" controls class="audio-player" />
      </div>
    </template>

    <!-- Archive / Unknown -->
    <template v-else>
      <div class="preview-placeholder">
        <span class="placeholder-icon">{{ info?.type === 'archive' ? '📦' : '📄' }}</span>
      </div>
    </template>
  </div>

  <!-- ── Zoom modal ──────────────────────────────────────────────────────── -->
  <Teleport to="body">
    <div v-if="showZoom" class="zoom-overlay" @click="closeZoom">
      <div class="zoom-panel" @click.stop>

        <!-- Header -->
        <div class="zoom-header">
          <template v-if="hasPreview">
            <button class="toggle-btn" :class="{ active: showOriginal }" @click="showOriginal = true">
              元画像<span class="toggle-size">{{ formatBytes(info.size) }}</span>
            </button>
            <button class="toggle-btn" :class="{ active: !showOriginal }" @click="showOriginal = false">
              処理後
              <span v-if="store.previewState" class="toggle-size">
                {{ formatBytes(store.previewState.outputSize) }}
                <span v-if="reductionBadge" class="toggle-ratio">{{ reductionBadge }}</span>
              </span>
            </button>
          </template>
          <span v-else class="zoom-header-title">プレビュー</span>
          <button class="zoom-close-btn" @click="closeZoom">✕</button>
        </div>

        <!-- Image area with zoom/pan -->
        <div
          ref="zoomWrapEl"
          class="zoom-img-wrap"
          :style="{ cursor: zoomCursor }"
          @wheel.prevent="onWheelZoom"
          @mousedown="onMouseDown"
          @mousemove="onMouseMove"
          @dblclick="onDblClick"
        >
          <img
            v-if="displaySrc"
            ref="zoomImgEl"
            :src="displaySrc"
            class="zoom-img"
            :style="imgZoomStyle"
            alt="zoom"
            draggable="false"
            @load="onImgLoad"
          />
        </div>

        <!-- Footer: zoom level + tips -->
        <div class="zoom-footer">
          <span class="zoom-pct">{{ zoomPct }}</span>
          <span class="zoom-tips">スクロール: ズーム &nbsp;|&nbsp; ドラッグ: 移動 &nbsp;|&nbsp; ダブルクリック: 2倍 / リセット</span>
          <button class="zoom-reset-btn" @click="resetZoom">リセット</button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.preview-wrap {
  width: 100%;
  background: var(--bg);
  border-radius: 10px;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: flex-start;
  position: relative;
  min-height: 80px;
}

/* Toggle bar */
.preview-toggle-bar {
  display: flex;
  width: 100%;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.toggle-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 12px;
  background: transparent;
  border: none;
  cursor: pointer;
  font-size: 0.78rem;
  font-weight: 500;
  color: var(--muted);
  transition: background 0.12s, color 0.12s;
}

.toggle-btn:first-child { border-right: 1px solid var(--border); }
.toggle-btn.active { background: var(--surface); color: var(--text); font-weight: 600; }
.toggle-btn:disabled { cursor: default; opacity: 0.5; }
.toggle-size { font-size: 0.72rem; font-weight: 400; color: var(--muted); }
.toggle-btn.active .toggle-size { color: var(--muted); }
.toggle-ratio { margin-left: 2px; color: #22c55e; font-weight: 600; }
.muted { color: var(--muted); }

/* Spinner overlay */
.spinner-overlay {
  position: absolute;
  inset: 0;
  top: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.08);
  z-index: 2;
}

.spinner {
  width: 24px;
  height: 24px;
  border: 2.5px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
}

@keyframes spin { to { transform: rotate(360deg); } }

/* Image (thumbnail) */
.img-container {
  width: 100%;
  position: relative;
  cursor: zoom-in;
}

.zoom-hint {
  display: none;
  position: absolute;
  bottom: 6px;
  right: 8px;
  font-size: 0.68rem;
  background: rgba(0, 0, 0, 0.55);
  color: #fff;
  padding: 2px 6px;
  border-radius: 4px;
  pointer-events: none;
}

.img-container:hover .zoom-hint { display: block; }

.preview-img {
  width: 100%;
  max-height: 200px;
  object-fit: contain;
  display: block;
}

.preview-video {
  width: 100%;
  max-height: 200px;
  display: block;
  background: #000;
}

.preview-audio {
  width: 100%;
  padding: 20px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.audio-player { width: 100%; }

.preview-placeholder {
  padding: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.placeholder-icon { font-size: 3rem; opacity: 0.5; }

/* ── Zoom modal ─────────────────────────────────────────────────────────── */

.zoom-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.82);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 200;
  padding: 16px;
}

.zoom-panel {
  background: var(--surface);
  border-radius: 12px;
  width: min(96vw, 1400px);
  height: min(94vh, 1000px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* Zoom header */
.zoom-header {
  display: flex;
  align-items: center;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.zoom-header .toggle-btn { border-right: 1px solid var(--border); }

.zoom-header-title {
  flex: 1;
  font-size: 0.82rem;
  color: var(--muted);
  padding: 0 14px;
}

.zoom-close-btn {
  flex-shrink: 0;
  padding: 9px 16px;
  background: none;
  border: none;
  border-left: 1px solid var(--border);
  color: var(--muted);
  cursor: pointer;
  font-size: 0.9rem;
  transition: color 0.12s;
  margin-left: auto;
}

.zoom-close-btn:hover { color: var(--text); }

/* Zoom image area (scrollable when zoomed) */
.zoom-img-wrap {
  flex: 1;
  overflow: auto;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--bg);
  user-select: none;
}

.zoom-img {
  max-width: 100%;
  max-height: 100%;
  object-fit: contain;
  display: block;
  pointer-events: none;
}

/* Zoom footer */
.zoom-footer {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 7px 14px;
  border-top: 1px solid var(--border);
  flex-shrink: 0;
  background: var(--surface);
}

.zoom-pct {
  font-family: monospace;
  font-size: 0.82rem;
  font-weight: 700;
  color: var(--text);
  min-width: 44px;
}

.zoom-tips {
  font-size: 0.72rem;
  color: var(--muted);
  flex: 1;
}

.zoom-reset-btn {
  padding: 4px 12px;
  font-size: 0.75rem;
  background: transparent;
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--muted);
  cursor: pointer;
  transition: color 0.12s, border-color 0.12s;
}

.zoom-reset-btn:hover { color: var(--text); border-color: var(--border-hover); }
</style>
