<script setup lang="ts">
import { computed, ref, watch, nextTick } from 'vue'
import { useProcessorStore } from '../stores/processor'

const store = useProcessorStore()

const status = computed(() => store.ffmpegStatus)
const dl = computed(() => store.dlProgress)
const log = computed(() => store.dlLog)

const isDownloading = computed(() => store.phase === 'downloading')

// Auto-scroll the log to the bottom as new lines arrive
const logBox = ref<HTMLElement | null>(null)
watch(() => log.value.length, async () => {
  await nextTick()
  if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight
})

const statusLabel = computed(() => {
  if (!status.value?.available) return 'ffmpeg が見つかりません'
  return '動作する ffmpeg が見つかりましたが、一部のコーデックが不足しています'
})

const progressLabel = computed(() => {
  if (!dl.value) return 'ダウンロードを準備中...'
  const p = dl.value
  if (p.phase === 'downloading_ffmpeg')  return `ffmpeg をダウンロード中 (${p.percent.toFixed(0)}%)`
  if (p.phase === 'downloading_ffprobe') return `ffprobe をダウンロード中 (${p.percent.toFixed(0)}%)`
  if (p.phase === 'extracting')  return '展開中...'
  if (p.phase === 'verifying')   return '動作確認中...'
  if (p.phase === 'done')        return '完了'
  return p.message
})

const overallPercent = computed(() => {
  if (!dl.value) return 0
  const p = dl.value
  if (p.phase === 'downloading_ffmpeg')  return p.percent * 0.45
  if (p.phase === 'downloading_ffprobe') return 45 + p.percent * 0.45
  if (p.phase === 'extracting')  return 90 + p.percent * 0.08
  if (p.phase === 'verifying')   return 98
  if (p.phase === 'done')        return 100
  return 0
})
</script>

<template>
  <div class="setup-view">
    <div class="setup-card">
      <!-- Icon -->
      <div class="setup-icon">
        <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M12 2L2 7l10 5 10-5-10-5z"/>
          <path d="M2 17l10 5 10-5M2 12l10 5 10-5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </div>

      <template v-if="!isDownloading">
        <h2 class="setup-title">ffmpeg のセットアップが必要です</h2>
        <p class="setup-reason">{{ statusLabel }}</p>
        <p class="setup-desc">
          AnyCrunch は処理エンジンとして ffmpeg を使用します。<br>
          WebP・AVIF 等のコーデックを含む
          <a href="https://github.com/homebrew-ffmpeg/homebrew-ffmpeg" target="_blank">homebrew-ffmpeg/ffmpeg</a>
          tap をインストールします。
        </p>

        <div class="setup-meta">
          <span class="meta-item">方法: brew tap homebrew-ffmpeg/ffmpeg → brew install --with-webp</span>
          <span class="meta-item warn">⚠ ソースからコンパイルのため 30〜60 分かかる場合があります</span>
        </div>

        <template v-if="status?.downloadSupported">
          <div class="setup-actions">
            <button
              v-if="status?.available"
              class="btn-secondary"
              @click="store.skipSetup()"
            >
              このまま使う（一部機能が制限される可能性あり）
            </button>
            <button class="btn-primary" @click="store.downloadFfmpeg()">
              インストールを開始（時間がかかります）
            </button>
          </div>
        </template>

        <template v-else>
          <!-- Linux: manual install guidance -->
          <div class="install-guide">
            <p class="guide-label">パッケージマネージャでインストールしてください：</p>
            <div class="code-block">
              <code>sudo apt install ffmpeg</code>
              <code>sudo pacman -S ffmpeg</code>
              <code>sudo dnf install ffmpeg</code>
            </div>
          </div>
          <div class="setup-actions">
            <button class="btn-primary" @click="store.skipSetup()">インストール済み、続ける</button>
          </div>
        </template>

        <p v-if="store.error" class="error-msg">{{ store.error }}</p>
      </template>

      <template v-else>
        <h2 class="setup-title">インストール中...</h2>
        <p class="setup-reason">{{ progressLabel }}</p>

        <div class="bar-track">
          <div class="bar-fill" :style="{ width: `${overallPercent}%` }" />
        </div>
        <p class="percent-label">{{ overallPercent.toFixed(0) }}%</p>

        <!-- Live command log -->
        <div v-if="log.length" ref="logBox" class="log-box">
          <div v-for="(line, i) in log" :key="i" class="log-line">{{ line }}</div>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.setup-view {
  width: 100%;
  max-width: 520px;
  margin: auto;
}

.setup-card {
  background: var(--surface);
  border-radius: 16px;
  padding: 40px 36px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  text-align: center;
}

.setup-icon {
  color: var(--accent);
  margin-bottom: 4px;
}

.setup-title {
  font-size: 1.3rem;
  font-weight: 700;
  color: var(--text);
  margin: 0;
}

.setup-reason {
  font-size: 0.875rem;
  color: #f59e0b;
  margin: 0;
  background: rgba(245, 158, 11, 0.1);
  padding: 6px 14px;
  border-radius: 99px;
}

.setup-desc {
  font-size: 0.875rem;
  color: var(--muted);
  line-height: 1.6;
  margin: 0;
}

.setup-meta {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 0.8rem;
  color: var(--muted);
  background: var(--bg);
  padding: 12px 20px;
  border-radius: 10px;
  width: 100%;
  text-align: left;
}

.meta-item::before { content: '• '; }
.meta-item.warn { color: #f59e0b; }
.setup-desc a { color: var(--accent); text-decoration: underline; }

.setup-actions {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: 100%;
  margin-top: 4px;
}

.btn-primary {
  padding: 13px 28px;
  background: var(--accent);
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.95rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s;
  width: 100%;
}

.btn-primary:hover { opacity: 0.85; }

.btn-secondary {
  padding: 10px 20px;
  background: transparent;
  color: var(--muted);
  border: 2px solid var(--border);
  border-radius: 8px;
  font-size: 0.8rem;
  cursor: pointer;
  transition: color 0.15s, border-color 0.15s;
  width: 100%;
}

.btn-secondary:hover { color: var(--text); border-color: var(--border-hover); }

/* Linux guide */
.install-guide {
  width: 100%;
  text-align: left;
}

.guide-label {
  font-size: 0.8rem;
  color: var(--muted);
  margin: 0 0 8px;
}

.code-block {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.code-block code {
  font-family: monospace;
  font-size: 0.85rem;
  background: var(--bg);
  padding: 6px 12px;
  border-radius: 6px;
  color: var(--text);
}

/* Progress */
.bar-track {
  width: 100%;
  height: 8px;
  background: var(--border);
  border-radius: 99px;
  overflow: hidden;
}

.bar-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 99px;
  transition: width 0.3s ease;
}

.percent-label {
  font-size: 0.9rem;
  font-weight: 600;
  color: var(--text);
  margin: 0;
}

.log-box {
  width: 100%;
  max-height: 180px;
  overflow-y: auto;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px;
  text-align: left;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.72rem;
  line-height: 1.5;
  color: var(--muted);
}

.log-line {
  white-space: pre-wrap;
  word-break: break-all;
}

.error-msg {
  font-size: 0.8rem;
  color: #ef4444;
  margin: 0;
  word-break: break-all;
}
</style>
