<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { setupFileDrop, openFilePicker } from '../composables/useFileDrop'

const emit = defineEmits<{ file: [f: File | string] }>()

const dropEl = ref<HTMLElement | null>(null)
const isDragOver = ref(false)
let cleanup: (() => void) | null = null

onMounted(async () => {
  if (dropEl.value) {
    cleanup = await setupFileDrop(dropEl.value, (f) => emit('file', f), isDragOver)
  }
})

onUnmounted(() => cleanup?.())

async function onBrowse() {
  await openFilePicker((f) => emit('file', f))
}
</script>

<template>
  <div
    ref="dropEl"
    class="drop-zone"
    :class="{ 'drag-over': isDragOver }"
    @click="onBrowse"
  >
    <div class="drop-inner">
      <div class="drop-icon">
        <svg width="56" height="56" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M12 16V4m0 0L8 8m4-4 4 4" stroke-linecap="round" stroke-linejoin="round"/>
          <path d="M20 16v2a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-2" stroke-linecap="round"/>
        </svg>
      </div>
      <p class="drop-title">ファイルをここにドロップ</p>
      <p class="drop-sub">画像・動画・音声・アーカイブに対応</p>
      <button class="browse-btn" @click.stop="onBrowse">ファイルを選択</button>
    </div>
  </div>
</template>

<style scoped>
.drop-zone {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 2px dashed var(--border);
  border-radius: 16px;
  cursor: pointer;
  transition: border-color 0.2s, background-color 0.2s;
  user-select: none;
}

.drop-zone:hover,
.drop-zone.drag-over {
  border-color: var(--accent);
  background-color: var(--accent-tint);
}

.drop-inner {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  pointer-events: none;
}

.drop-icon {
  color: var(--muted);
  transition: color 0.2s;
}

.drop-zone:hover .drop-icon,
.drop-zone.drag-over .drop-icon {
  color: var(--accent);
}

.drop-title {
  font-size: 1.25rem;
  font-weight: 600;
  color: var(--text);
  margin: 0;
}

.drop-sub {
  font-size: 0.875rem;
  color: var(--muted);
  margin: 0;
}

.browse-btn {
  pointer-events: all;
  margin-top: 8px;
  padding: 10px 24px;
  background: var(--accent);
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.9rem;
  font-weight: 500;
  cursor: pointer;
  transition: opacity 0.15s;
}

.browse-btn:hover {
  opacity: 0.85;
}
</style>
