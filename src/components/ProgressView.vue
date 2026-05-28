<script setup lang="ts">
import { computed } from 'vue'
import { useProcessorStore } from '../stores/processor'
import { formatEta } from '../lib/format'

const store = useProcessorStore()

const percent = computed(() => store.progress?.percent ?? 0)
const step = computed(() => {
  const s = store.progress?.currentStep
  if (!s || s === 'encoding') return 'エンコード中...'
  if (s === 'done') return '完了'
  return s
})
const eta = computed(() => {
  const e = store.progress?.eta
  if (e == null || e <= 0) return null
  return formatEta(e)
})

async function onCancel() {
  await store.cancel()
}
</script>

<template>
  <div class="progress-view">
    <div class="progress-card">
      <p class="file-name">{{ store.fileName }}</p>
      <p class="step-label">{{ step }}</p>

      <div class="bar-track">
        <div class="bar-fill" :style="{ width: `${percent}%` }" />
      </div>

      <div class="progress-meta">
        <span class="percent">{{ Math.round(percent) }}%</span>
        <span v-if="eta" class="eta">{{ eta }}</span>
      </div>

      <button class="cancel-btn" @click="onCancel">キャンセル</button>
    </div>
  </div>
</template>

<style scoped>
.progress-view {
  width: 100%;
  max-width: 480px;
  margin: auto;
  display: flex;
  align-items: center;
  justify-content: center;
}

.progress-card {
  width: 100%;
  background: var(--surface);
  border-radius: 16px;
  padding: 32px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  align-items: stretch;
}

.file-name {
  font-size: 0.9rem;
  color: var(--muted);
  margin: 0;
  text-align: center;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.step-label {
  font-size: 1.1rem;
  font-weight: 600;
  color: var(--text);
  margin: 0;
  text-align: center;
}

.bar-track {
  height: 8px;
  background: var(--border);
  border-radius: 99px;
  overflow: hidden;
}

.bar-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 99px;
  transition: width 0.4s ease;
}

.progress-meta {
  display: flex;
  justify-content: space-between;
  font-size: 0.85rem;
  color: var(--muted);
}

.percent {
  font-weight: 600;
  color: var(--text);
}

.cancel-btn {
  align-self: center;
  margin-top: 8px;
  padding: 8px 24px;
  background: transparent;
  color: var(--muted);
  border: 2px solid var(--border);
  border-radius: 8px;
  font-size: 0.875rem;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
}

.cancel-btn:hover {
  color: #ef4444;
  border-color: #ef4444;
}
</style>
