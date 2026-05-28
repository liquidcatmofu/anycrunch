<script setup lang="ts">
import { computed, ref } from 'vue'
import { useProcessorStore } from '../stores/processor'
import { formatBytes, reductionPercent, formatDuration } from '../lib/format'
import { isTauri, invoke } from '@tauri-apps/api/core'

const store = useProcessorStore()
const result = computed(() => store.result!)

const reduced = computed(
  () => result.value.originalSize - result.value.outputSize > 0
)

const ratio = computed(() =>
  reductionPercent(result.value.originalSize, result.value.outputSize)
)

const elapsed = computed(() =>
  formatDuration(result.value.duration / 1000)
)

async function revealInFinder() {
  if (!isTauri()) return
  const { revealItemInDir } = await import('@tauri-apps/plugin-opener')
  revealItemInDir(result.value.outputPath)
}

// ── Clipboard ──────────────────────────────────────────────────────────────

const pathCopied = ref(false)
const fileCopied = ref(false)

async function copyPath() {
  await navigator.clipboard.writeText(result.value.outputPath)
  pathCopied.value = true
  setTimeout(() => { pathCopied.value = false }, 1500)
}

async function copyFile() {
  if (!isTauri()) return
  await invoke('copy_file_to_clipboard', { path: result.value.outputPath })
  fileCopied.value = true
  setTimeout(() => { fileCopied.value = false }, 1500)
}
</script>

<template>
  <div class="result-view">
    <div class="result-card">
      <div class="result-icon" :class="reduced ? 'success' : 'warn'">
        <svg v-if="reduced" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M20 6L9 17l-5-5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        <svg v-else width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10"/>
          <path d="M12 8v4M12 16h.01" stroke-linecap="round"/>
        </svg>
      </div>

      <h2 class="result-title">{{ reduced ? '圧縮完了' : '変換完了' }}</h2>

      <!-- Size comparison -->
      <div class="size-compare">
        <div class="size-col">
          <span class="size-label">元のサイズ</span>
          <span class="size-val">{{ formatBytes(result.originalSize) }}</span>
        </div>
        <div class="arrow">→</div>
        <div class="size-col">
          <span class="size-label">出力サイズ</span>
          <span class="size-val accent">{{ formatBytes(result.outputSize) }}</span>
        </div>
        <div class="ratio-badge" :class="reduced ? 'ratio-green' : 'ratio-gray'">
          {{ ratio }}
        </div>
      </div>

      <!-- Output path -->
      <div class="output-path">
        <div class="path-header">
          <span class="path-label">保存先</span>
          <button class="btn-copy-path" @click="copyPath">
            {{ pathCopied ? 'コピー済み' : 'パスをコピー' }}
          </button>
        </div>
        <span class="path-value">{{ result.outputPath }}</span>
      </div>

      <div class="meta-row">
        <span class="meta-item">処理時間 {{ elapsed }}</span>
      </div>

      <!-- Actions -->
      <div class="actions">
        <button v-if="isTauri()" class="btn-secondary" @click="copyFile">
          {{ fileCopied ? 'コピー済み' : 'ファイルをコピー' }}
        </button>
        <button v-if="isTauri()" class="btn-secondary" @click="revealInFinder">
          Finderで表示
        </button>
        <button class="btn-primary" @click="store.reset()">別のファイルを処理</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.result-view {
  width: 100%;
  max-width: 480px;
  margin: auto;
  display: flex;
  align-items: center;
  justify-content: center;
}

.result-card {
  width: 100%;
  background: var(--surface);
  border-radius: 16px;
  padding: 32px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 20px;
}

.result-icon {
  width: 64px;
  height: 64px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
}

.result-icon.success { background: rgba(34, 197, 94, 0.15); color: #22c55e; }
.result-icon.warn    { background: rgba(245, 158, 11, 0.15); color: #f59e0b; }

.result-title {
  font-size: 1.4rem;
  font-weight: 700;
  color: var(--text);
  margin: 0;
}

.size-compare {
  display: flex;
  align-items: center;
  gap: 16px;
  width: 100%;
  background: var(--bg);
  border-radius: 12px;
  padding: 16px 20px;
}

.size-col {
  display: flex;
  flex-direction: column;
  gap: 4px;
  flex: 1;
  text-align: center;
}

.size-label {
  font-size: 0.75rem;
  color: var(--muted);
}

.size-val {
  font-size: 1.1rem;
  font-weight: 700;
  color: var(--text);
}

.size-val.accent { color: var(--accent); }

.arrow {
  color: var(--muted);
  font-size: 1.2rem;
}

.ratio-badge {
  padding: 4px 10px;
  border-radius: 99px;
  font-size: 0.85rem;
  font-weight: 700;
  white-space: nowrap;
}

.ratio-green { background: rgba(34, 197, 94, 0.15); color: #22c55e; }
.ratio-gray  { background: var(--border); color: var(--muted); }

.output-path {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.path-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.path-label {
  font-size: 0.75rem;
  color: var(--muted);
}

.btn-copy-path {
  font-size: 0.72rem;
  padding: 2px 8px;
  background: transparent;
  color: var(--accent);
  border: 1px solid var(--accent);
  border-radius: 4px;
  cursor: pointer;
  transition: opacity 0.15s;
}

.btn-copy-path:hover { opacity: 0.75; }

.path-value {
  font-size: 0.8rem;
  font-family: monospace;
  color: var(--text);
  word-break: break-all;
  background: var(--bg);
  padding: 8px 12px;
  border-radius: 8px;
}

.meta-row {
  display: flex;
  gap: 16px;
  font-size: 0.8rem;
  color: var(--muted);
}

.actions {
  display: flex;
  gap: 12px;
  width: 100%;
  justify-content: center;
  flex-wrap: wrap;
}

.btn-primary {
  padding: 12px 28px;
  background: var(--accent);
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.9rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s;
}

.btn-primary:hover { opacity: 0.85; }

.btn-secondary {
  padding: 12px 20px;
  background: transparent;
  color: var(--muted);
  border: 2px solid var(--border);
  border-radius: 8px;
  font-size: 0.875rem;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}

.btn-secondary:hover {
  color: var(--text);
  border-color: var(--border-hover);
}
</style>
