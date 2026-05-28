<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { isTauri } from '@tauri-apps/api/core'
import { useProcessorStore } from './stores/processor'
import DropZone from './components/DropZone.vue'
import RecommendView from './components/RecommendView.vue'
import ProgressView from './components/ProgressView.vue'
import ResultView from './components/ResultView.vue'
import FfmpegSetup from './components/FfmpegSetup.vue'
import CodecStatus from './components/CodecStatus.vue'

const store = useProcessorStore()
const showCodecStatus = ref(false)

onMounted(() => {
  store.initFfmpeg()
})
</script>

<template>
  <div class="app">
    <header class="app-header">
      <span class="logo">AnyCrunch</span>
      <button
        v-if="isTauri()"
        class="codec-btn"
        title="コーデック対応状況"
        @click="showCodecStatus = true"
      >
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="10"/>
          <path d="M12 8v4M12 16h.01" stroke-linecap="round"/>
        </svg>
      </button>
    </header>

    <Teleport to="body">
      <CodecStatus v-if="showCodecStatus" @close="showCodecStatus = false" />
    </Teleport>

    <main class="app-main">
      <Transition name="fade" mode="out-in">
        <DropZone
          v-if="store.phase === 'idle'"
          key="drop"
          @file="store.loadFile"
        />

        <div v-else-if="store.phase === 'analyzing'" key="analyzing" class="centered">
          <div class="spinner" />
          <p class="analyzing-text">解析中...</p>
        </div>

        <RecommendView
          v-else-if="store.phase === 'recommend'"
          key="recommend"
        />

        <ProgressView
          v-else-if="store.phase === 'processing'"
          key="progress"
        />

        <ResultView
          v-else-if="store.phase === 'done'"
          key="result"
        />

        <FfmpegSetup
          v-else-if="store.phase === 'setup' || store.phase === 'downloading'"
          key="setup"
        />

        <div v-else-if="store.phase === 'error'" key="error" class="centered error-view">
          <p class="error-icon">✕</p>
          <p class="error-msg">{{ store.error }}</p>
          <button class="btn-primary" @click="store.reset()">最初からやり直す</button>
        </div>
      </Transition>
    </main>
  </div>
</template>

<style>
/* ── Design tokens ── */
:root {
  --bg:           #111827;
  --surface:      #1f2937;
  --border:       #374151;
  --border-hover: #6b7280;
  --text:         #f9fafb;
  --muted:        #9ca3af;
  --accent:       #3b82f6;
  --accent-tint:  rgba(59, 130, 246, 0.1);
}

@media (prefers-color-scheme: light) {
  :root {
    --bg:           #f3f4f6;
    --surface:      #ffffff;
    --border:       #e5e7eb;
    --border-hover: #9ca3af;
    --text:         #111827;
    --muted:        #6b7280;
    --accent:       #2563eb;
    --accent-tint:  rgba(37, 99, 235, 0.08);
  }
}

*, *::before, *::after { box-sizing: border-box; }

body {
  margin: 0;
  background: var(--bg);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  font-size: 16px;
  line-height: 1.5;
  -webkit-font-smoothing: antialiased;
  overflow: hidden;
}

button { font-family: inherit; }
</style>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  width: 100vw;
}

.app-header {
  padding: 14px 24px;
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: center;
}

.logo {
  font-size: 1rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  color: var(--accent);
  flex: 1;
}

.codec-btn {
  background: none;
  border: none;
  color: var(--muted);
  cursor: pointer;
  padding: 6px;
  border-radius: 6px;
  display: flex;
  align-items: center;
  transition: color 0.15s, background-color 0.15s;
}
.codec-btn:hover {
  color: var(--text);
  background: var(--border);
}

.app-main {
  flex: 1;
  overflow-y: auto;
  padding: 32px 24px;
  display: flex;
  flex-direction: column;
  align-items: center;
}

.centered {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
}

.spinner {
  width: 40px;
  height: 40px;
  border: 3px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin { to { transform: rotate(360deg); } }

.analyzing-text {
  color: var(--muted);
  font-size: 0.95rem;
  margin: 0;
}

.error-view { max-width: 400px; text-align: center; }

.error-icon { font-size: 2.5rem; color: #ef4444; margin: 0; }

.error-msg {
  font-size: 0.9rem;
  color: var(--muted);
  margin: 0;
  word-break: break-all;
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

.fade-enter-active, .fade-leave-active { transition: opacity 0.2s ease; }
.fade-enter-from, .fade-leave-to { opacity: 0; }

/* DropZone fills available vertical space */
:deep(.drop-zone) {
  width: 100%;
  min-height: min(60vh, 480px);
}

/* Short-content views (progress, result, error) self-center */
.centered,
.error-view {
  margin: auto;
}
</style>
