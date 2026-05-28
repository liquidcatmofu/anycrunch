<script setup lang="ts">
import { ref, computed, onMounted, nextTick, watch } from 'vue'
import { invoke, isTauri, Channel } from '@tauri-apps/api/core'
import { useProcessorStore } from '../stores/processor'

interface FfmpegInfo {
  version: string
  path: string
  encoders: string[]
  decoders: string[]
  buildFlags: string[]
}

interface EncoderCaps {
  name: string
  found: boolean
  pixFmts: string[]
  supportsAlpha: boolean
  supportsHighDepth: boolean
}

interface AvifencStatus {
  available: boolean
  avifdecAvailable: boolean
  path: string | null
  version: string | null
  installSupported: boolean
}

interface AvifInstallProgress {
  phase: string
  percent: number
  message: string
}

const emit = defineEmits<{ close: [] }>()
const store = useProcessorStore()

const info = ref<FfmpegInfo | null>(null)
const loading = ref(true)
const error = ref<string | null>(null)

// Per-encoder capability cache, keyed by encoder name
const caps = ref<Record<string, EncoderCaps>>({})

// ── libavif state ──────────────────────────────────────────────────────────

const avifStatus = ref<AvifencStatus | null>(null)
const avifInstalling = ref(false)
const avifInstallPercent = ref(0)
const avifInstallLog = ref<string[]>([])
const avifInstallError = ref<string | null>(null)
const avifLogBox = ref<HTMLElement | null>(null)

const customEncPath = ref(store.settings.avifencPath ?? '')
const customDecPath = ref(store.settings.avifdecPath ?? '')
const pathSaving = ref(false)

watch(() => avifInstallLog.value.length, async () => {
  await nextTick()
  if (avifLogBox.value) avifLogBox.value.scrollTop = avifLogBox.value.scrollHeight
})

async function refreshAvifStatus() {
  avifStatus.value = await invoke<AvifencStatus>('check_avifenc').catch(() => null)
  if (avifStatus.value) store.avifencStatus = avifStatus.value
}

async function applyAvifPaths() {
  pathSaving.value = true
  await store.updateAvifPaths(customEncPath.value, customDecPath.value)
  avifStatus.value = store.avifencStatus
  pathSaving.value = false
}

async function installAvifenc() {
  avifInstalling.value = true
  avifInstallPercent.value = 0
  avifInstallLog.value = []
  avifInstallError.value = null

  const channel = new Channel<AvifInstallProgress>()
  channel.onmessage = (p) => {
    avifInstallPercent.value = p.percent
    if (p.message) avifInstallLog.value.push(p.message)
  }

  try {
    await invoke('install_avifenc', { onProgress: channel })
    await refreshAvifStatus()
  } catch (e) {
    avifInstallError.value = String(e)
  } finally {
    avifInstalling.value = false
  }
}

onMounted(async () => {
  try {
    const [ffInfo] = await Promise.all([
      invoke<FfmpegInfo>('get_ffmpeg_info'),
      refreshAvifStatus(),
    ])
    info.value = ffInfo
    await loadCaps()
  } catch (e) {
    error.value = String(e)
  } finally {
    loading.value = false
  }
})

// Fetch pixel-format capabilities for every video/image encoder that's present.
// Audio encoders are skipped (alpha/bit-depth don't apply).
async function loadCaps() {
  if (!info.value) return
  const wanted = new Set<string>()
  for (const group of GROUPS) {
    if (group.label === '音声') continue
    for (const codec of group.codecs) {
      const enc = codec.encoders.find(e => info.value!.encoders.includes(e))
      if (enc) wanted.add(enc)
    }
  }
  const results = await Promise.all(
    [...wanted].map(async (name) => {
      try {
        return await invoke<EncoderCaps>('get_encoder_caps', { name })
      } catch {
        return null
      }
    })
  )
  const map: Record<string, EncoderCaps> = {}
  for (const r of results) {
    if (r) map[r.name] = r
  }
  caps.value = map
}

// ── Codec definitions ──────────────────────────────────────────────────────

interface CodecDef {
  label: string
  note?: string
  /** encoder names to look for (any match = available) */
  encoders: string[]
  /** decoder names to look for */
  decoders: string[]
  /** shown if missing */
  missing?: string
}

interface Group {
  label: string
  icon: string
  codecs: CodecDef[]
}

const GROUPS: Group[] = [
  {
    label: '動画',
    icon: '🎬',
    codecs: [
      {
        label: 'H.264',
        encoders: ['libx264', 'h264_videotoolbox', 'h264_nvenc', 'h264_amf', 'h264_qsv'],
        decoders: ['h264', 'h264_cuvid'],
      },
      {
        label: 'H.265 / HEVC',
        encoders: ['libx265', 'hevc_videotoolbox', 'hevc_nvenc', 'hevc_amf', 'hevc_qsv'],
        decoders: ['hevc'],
      },
      {
        label: 'AV1',
        encoders: ['libsvtav1', 'libaom-av1', 'av1_nvenc', 'av1_amf', 'av1_qsv'],
        decoders: ['libdav1d', 'av1', 'av1_cuvid'],
      },
      {
        label: 'VP9',
        encoders: ['libvpx-vp9'],
        decoders: ['vp9', 'libvpx-vp9'],
      },
    ],
  },
  {
    label: '画像',
    icon: '🖼',
    codecs: [
      {
        label: 'JPEG',
        encoders: ['mjpeg'],
        decoders: ['mjpeg'],
      },
      {
        label: 'PNG',
        encoders: ['png'],
        decoders: ['png'],
      },
      {
        label: 'WebP',
        encoders: ['libwebp', 'webp'],
        decoders: ['webp'],
        missing: 'brew tap homebrew-ffmpeg/ffmpeg\nbrew install homebrew-ffmpeg/ffmpeg/ffmpeg --with-webp',
      },
      {
        label: 'AVIF',
        encoders: ['libsvtav1', 'libaom-av1'],
        decoders: ['libdav1d', 'av1'],
        note: 'AV1 エンコーダで代替',
      },
      {
        label: 'HEIC / HEIF',
        encoders: [],
        decoders: ['hevc'],
        note: 'デコードのみ（変換元として使用可）',
      },
    ],
  },
  {
    label: '音声',
    icon: '🎵',
    codecs: [
      {
        label: 'AAC',
        encoders: ['aac', 'aac_at', 'libfdk_aac'],
        decoders: ['aac'],
      },
      {
        label: 'MP3',
        encoders: ['libmp3lame'],
        decoders: ['mp3'],
      },
      {
        label: 'Opus',
        encoders: ['libopus', 'opus'],
        decoders: ['opus', 'libopus'],
      },
      {
        label: 'FLAC',
        encoders: ['flac'],
        decoders: ['flac'],
      },
    ],
  },
]

// ── Status computation ─────────────────────────────────────────────────────

function findEncoder(def: CodecDef): string | null {
  if (!info.value) return null
  return def.encoders.find(e => info.value!.encoders.includes(e)) ?? null
}

function canDecode(def: CodecDef): boolean {
  if (!info.value) return false
  return def.decoders.some(d => info.value!.decoders.includes(d))
}

type Status = 'ok' | 'decode-only' | 'missing'

function statusOf(def: CodecDef): Status {
  const enc = findEncoder(def)
  if (enc) return 'ok'
  if (def.encoders.length === 0 && canDecode(def)) return 'decode-only'
  if (canDecode(def)) return 'decode-only'
  return 'missing'
}

const hwEncoders = new Set([
  'h264_videotoolbox', 'hevc_videotoolbox',
  'h264_nvenc', 'hevc_nvenc', 'av1_nvenc',
  'h264_amf', 'hevc_amf', 'av1_amf',
  'h264_qsv', 'hevc_qsv', 'av1_qsv',
])

function isHw(enc: string): boolean {
  return hwEncoders.has(enc)
}

/** Capability chips for a codec's active encoder (alpha, bit depth) */
function capChips(def: CodecDef, groupLabel: string): { label: string; kind: 'yes' | 'no' }[] {
  const enc = findEncoder(def)
  if (!enc) return []
  const c = caps.value[enc]
  if (!c) return []

  const chips: { label: string; kind: 'yes' | 'no' }[] = []
  // Alpha is only meaningful for image codecs
  if (groupLabel === '画像') {
    chips.push({ label: c.supportsAlpha ? '透過対応' : '透過なし', kind: c.supportsAlpha ? 'yes' : 'no' })
  }
  if (c.supportsHighDepth) {
    chips.push({ label: '10bit+', kind: 'yes' })
  }
  return chips
}

const missingCount = computed(() =>
  GROUPS.flatMap(g => g.codecs).filter(c => statusOf(c) === 'missing' && c.encoders.length > 0).length
)

// ── Capability gaps & reinstall ─────────────────────────────────────────────

const hasWebp = computed(() => {
  const enc = info.value?.encoders ?? []
  return enc.includes('libwebp') || enc.includes('webp')
})

/** Gaps that a reinstall can actually fix. AVIF-alpha is intentionally excluded:
 *  ffmpeg cannot produce transparent AVIF even with libaom-av1 (the wrapper
 *  doesn't expose alpha pixel formats), so reinstalling wouldn't help. */
const gaps = computed<string[]>(() => {
  if (!info.value) return []
  const g: string[] = []
  if (!hasWebp.value) g.push('WebP（透過対応）')
  return g
})

// Reinstall is only meaningful on desktop where we drive Homebrew/the zip install.
const canReinstall = computed(() => isTauri() && gaps.value.length > 0)

function reinstall() {
  // do_brew_install auto-switches to `reinstall` when already present.
  store.downloadFfmpeg()
  emit('close') // close modal; FfmpegSetup shows the download progress
}

// ── Full encoder/decoder list ──────────────────────────────────────────────

type HwVendor = 'nvidia' | 'amd' | 'intel' | 'apple' | 'hw-other' | null

function hwVendor(name: string): HwVendor {
  if (/nvenc|cuvid/.test(name))                         return 'nvidia'
  if (/amf/.test(name))                                 return 'amd'
  if (/qsv/.test(name))                                 return 'intel'
  if (/videotoolbox/.test(name))                        return 'apple'
  if (/vaapi|vulkan|d3d12va|dxva|v4l2m2m/.test(name))  return 'hw-other'
  return null
}

const hwVendorLabel: Record<NonNullable<HwVendor>, string> = {
  nvidia:   'NVIDIA',
  amd:      'AMD',
  intel:    'Intel',
  apple:    'Apple',
  'hw-other': 'HW',
}
</script>

<template>
  <div class="overlay" @click.self="emit('close')">
    <div class="panel">
      <div class="panel-header">
        <h2 class="panel-title">コーデック対応状況</h2>
        <button class="close-btn" @click="emit('close')">✕</button>
      </div>

      <div v-if="loading" class="center-msg">
        <div class="spinner" />
      </div>

      <div v-else-if="error" class="center-msg error-msg">
        {{ error }}
      </div>

      <div v-else-if="info" class="panel-body">
        <!-- ffmpeg info -->
        <div class="ff-meta">
          <span class="ff-version">ffmpeg {{ info.version }}</span>
          <span class="ff-path">{{ info.path }}</span>
          <span v-if="missingCount > 0" class="missing-badge">
            {{ missingCount }} コーデック未対応
          </span>
        </div>

        <!-- Capability gap banner + reinstall -->
        <div v-if="canReinstall" class="gap-banner">
          <div class="gap-text">
            <span class="gap-title">⚠ 一部のコーデックが不足しています</span>
            <span class="gap-list">{{ gaps.join('、') }}</span>
            <span class="gap-note">全コーデック対応版を入れ直せます（ソースからコンパイルのため 30〜60 分）</span>
          </div>
          <button class="reinstall-btn" @click="reinstall">入れ直す</button>
        </div>

        <!-- Codec groups -->
        <div v-for="group in GROUPS" :key="group.label" class="group">
          <div class="group-label">
            <span>{{ group.icon }}</span>
            <span>{{ group.label }}</span>
          </div>

          <div class="codec-list">
            <div
              v-for="codec in group.codecs"
              :key="codec.label"
              class="codec-row"
              :class="statusOf(codec)"
            >
              <span class="codec-icon">
                <template v-if="statusOf(codec) === 'ok'">✅</template>
                <template v-else-if="statusOf(codec) === 'decode-only'">🔵</template>
                <template v-else>❌</template>
              </span>

              <div class="codec-info">
                <span class="codec-label">{{ codec.label }}</span>
                <span v-if="codec.note" class="codec-note">{{ codec.note }}</span>

                <template v-if="statusOf(codec) === 'ok'">
                  <span class="tag-row">
                    <span class="encoder-tag" :class="{ hw: isHw(findEncoder(codec)!) }">
                      {{ findEncoder(codec) }}
                      <span v-if="isHw(findEncoder(codec)!)" class="hw-badge">HW</span>
                    </span>
                    <span
                      v-for="chip in capChips(codec, group.label)"
                      :key="chip.label"
                      class="cap-chip"
                      :class="chip.kind"
                    >{{ chip.label }}</span>
                  </span>
                </template>

                <template v-else-if="statusOf(codec) === 'decode-only'">
                  <span class="codec-note muted">エンコード不可・デコードのみ</span>
                </template>

                <template v-else>
                  <span class="codec-note missing">エンコード非対応</span>
                  <code v-if="codec.missing" class="install-hint">{{ codec.missing }}</code>
                </template>
              </div>
            </div>
          </div>
        </div>

        <!-- libavif section -->
        <div v-if="avifStatus" class="avif-section">
          <div class="group-label">
            <span>🖼</span>
            <span>libavif (AVIF ネイティブ処理)</span>
          </div>

          <!-- Installing -->
          <div v-if="avifInstalling" class="avif-installing">
            <p class="avif-install-label">インストール中... {{ avifInstallPercent.toFixed(0) }}%</p>
            <div class="bar-track">
              <div class="bar-fill" :style="{ width: `${avifInstallPercent}%` }" />
            </div>
            <div v-if="avifInstallLog.length" ref="avifLogBox" class="avif-log">
              <div v-for="(line, i) in avifInstallLog" :key="i" class="avif-log-line">{{ line }}</div>
            </div>
          </div>

          <!-- Status rows -->
          <div v-else class="codec-list">
            <!-- avifenc row -->
            <div class="codec-row" :class="avifStatus.available ? 'ok' : 'missing'">
              <span class="codec-icon">{{ avifStatus.available ? '✅' : '❌' }}</span>
              <div class="codec-info">
                <span class="codec-label">avifenc</span>
                <template v-if="avifStatus.available">
                  <span class="tag-row">
                    <span class="encoder-tag">{{ avifStatus.version ?? 'libavif' }}</span>
                    <span class="cap-chip yes">透過対応</span>
                    <span class="cap-chip yes">アルファ</span>
                  </span>
                  <span class="codec-note">{{ avifStatus.path }}</span>
                </template>
                <template v-else>
                  <span class="codec-note missing">未インストール — 透過 AVIF は FFmpeg にフォールバック（アルファ非対応）</span>
                </template>
              </div>
            </div>

            <!-- avifdec row -->
            <div class="codec-row" :class="avifStatus.avifdecAvailable ? 'ok' : 'missing'">
              <span class="codec-icon">{{ avifStatus.avifdecAvailable ? '✅' : '❌' }}</span>
              <div class="codec-info">
                <span class="codec-label">avifdec</span>
                <span v-if="!avifStatus.avifdecAvailable" class="codec-note missing">
                  未インストール — 透過 AVIF の入力デコードに必要
                </span>
              </div>
            </div>

            <!-- Install button / guide -->
            <template v-if="!avifStatus.available">
              <div v-if="avifStatus.installSupported" class="avif-install-row">
                <p class="avif-install-note">
                  <code>brew install libavif</code> で avifenc と avifdec を一括インストールします
                </p>
                <button class="install-btn" @click="installAvifenc">インストール</button>
                <p v-if="avifInstallError" class="avif-error">{{ avifInstallError }}</p>
              </div>
              <div v-else class="avif-guide">
                <p class="guide-label">パッケージマネージャでインストールしてください：</p>
                <div class="guide-commands">
                  <code>brew install libavif</code>
                  <code>apt install libavif-bin</code>
                </div>
                <p class="guide-note">Windows では winget での導入は非対応です。<a href="https://github.com/AOMediaCodec/libavif/releases" target="_blank">libavif リリースページ</a>から手動でダウンロードしてください。</p>
              </div>
            </template>

            <!-- Custom path input (always shown) -->
            <div class="avif-custom-path">
              <p class="guide-label">手動インストールのパス指定：</p>
              <label class="path-row">
                <span class="path-label">avifenc</span>
                <input v-model="customEncPath" class="path-input" placeholder="例: C:\tools\avifenc.exe または /usr/local/bin/avifenc" />
              </label>
              <label class="path-row">
                <span class="path-label">avifdec</span>
                <input v-model="customDecPath" class="path-input" placeholder="例: C:\tools\avifdec.exe または /usr/local/bin/avifdec" />
              </label>
              <button class="path-apply-btn" :disabled="pathSaving" @click="applyAvifPaths">
                {{ pathSaving ? '確認中...' : '適用して確認' }}
              </button>
            </div>
          </div>
        </div>

        <!-- Full encoder / decoder list -->
        <details class="build-flags">
          <summary>全エンコーダ / デコーダ一覧</summary>
          <div class="full-codec-cols">
            <div class="full-codec-col">
              <p class="full-codec-heading">エンコーダ ({{ info.encoders.length }})</p>
              <div class="full-codec-chips">
                <span
                  v-for="enc in info.encoders"
                  :key="enc"
                  class="full-chip"
                  :class="hwVendor(enc) ? `hw-${hwVendor(enc)}` : ''"
                  :title="hwVendor(enc) ? hwVendorLabel[hwVendor(enc)!] : ''"
                >{{ enc }}</span>
              </div>
            </div>
            <div class="full-codec-col">
              <p class="full-codec-heading">デコーダ ({{ info.decoders.length }})</p>
              <div class="full-codec-chips">
                <span
                  v-for="dec in info.decoders"
                  :key="dec"
                  class="full-chip"
                  :class="hwVendor(dec) ? `hw-${hwVendor(dec)}` : ''"
                  :title="hwVendor(dec) ? hwVendorLabel[hwVendor(dec)!] : ''"
                >{{ dec }}</span>
              </div>
            </div>
          </div>
          <!-- Legend -->
          <div class="hw-legend">
            <span class="full-chip hw-nvidia">NVIDIA</span>
            <span class="full-chip hw-amd">AMD</span>
            <span class="full-chip hw-intel">Intel</span>
            <span class="full-chip hw-apple">Apple</span>
            <span class="full-chip hw-hw-other">その他HW</span>
          </div>
        </details>

        <!-- Build flags -->
        <details class="build-flags">
          <summary>ビルドフラグ ({{ info.buildFlags.length }})</summary>
          <div class="flags-list">
            <span v-for="f in info.buildFlags" :key="f" class="flag-tag">{{ f }}</span>
          </div>
        </details>
      </div>
    </div>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0,0,0,0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
  padding: 24px;
}

.panel {
  background: var(--surface);
  border-radius: 16px;
  width: 100%;
  max-width: 600px;
  max-height: 85vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 18px 24px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}

.panel-title {
  font-size: 1rem;
  font-weight: 700;
  margin: 0;
  color: var(--text);
}

.close-btn {
  background: none;
  border: none;
  color: var(--muted);
  cursor: pointer;
  font-size: 1rem;
  padding: 4px 8px;
  border-radius: 6px;
  transition: color 0.15s;
}
.close-btn:hover { color: var(--text); }

.panel-body {
  overflow-y: auto;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

/* ffmpeg meta */
.ff-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-wrap: wrap;
  background: var(--bg);
  padding: 10px 14px;
  border-radius: 10px;
}

.ff-version {
  font-weight: 700;
  font-size: 0.9rem;
  color: var(--text);
}

.ff-path {
  font-size: 0.75rem;
  font-family: monospace;
  color: var(--muted);
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.missing-badge {
  background: rgba(239, 68, 68, 0.12);
  color: #ef4444;
  font-size: 0.75rem;
  font-weight: 600;
  padding: 2px 8px;
  border-radius: 99px;
  white-space: nowrap;
}

/* Capability gap banner */
.gap-banner {
  display: flex;
  align-items: center;
  gap: 16px;
  background: rgba(245, 158, 11, 0.1);
  border: 1px solid rgba(245, 158, 11, 0.3);
  border-radius: 10px;
  padding: 14px 16px;
}

.gap-text {
  display: flex;
  flex-direction: column;
  gap: 3px;
  flex: 1;
  min-width: 0;
}

.gap-title {
  font-size: 0.85rem;
  font-weight: 700;
  color: #f59e0b;
}

.gap-list {
  font-size: 0.8rem;
  color: var(--text);
}

.gap-note {
  font-size: 0.72rem;
  color: var(--muted);
}

.reinstall-btn {
  flex-shrink: 0;
  padding: 10px 18px;
  background: #f59e0b;
  color: #000;
  border: none;
  border-radius: 8px;
  font-size: 0.85rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s;
}

.reinstall-btn:hover { opacity: 0.85; }

/* Groups */
.group-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8rem;
  font-weight: 600;
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  margin-bottom: 8px;
}

.codec-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.codec-row {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--bg);
}

.codec-icon { font-size: 1rem; flex-shrink: 0; margin-top: 1px; }

.codec-info {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}

.codec-label {
  font-size: 0.875rem;
  font-weight: 600;
  color: var(--text);
}

.codec-note {
  font-size: 0.75rem;
  color: var(--muted);
}

.codec-note.missing { color: #ef4444; }
.codec-note.muted   { color: var(--muted); }

.tag-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}

.encoder-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 0.72rem;
  font-family: monospace;
  background: rgba(59, 130, 246, 0.1);
  color: var(--accent);
  padding: 1px 6px;
  border-radius: 4px;
  width: fit-content;
}

.cap-chip {
  font-size: 0.68rem;
  font-weight: 600;
  padding: 1px 6px;
  border-radius: 99px;
}

.cap-chip.yes {
  background: rgba(34, 197, 94, 0.12);
  color: #22c55e;
}

.cap-chip.no {
  background: rgba(148, 163, 184, 0.15);
  color: var(--muted);
}

.encoder-tag.hw {
  background: rgba(34, 197, 94, 0.1);
  color: #22c55e;
}

.hw-badge {
  font-family: sans-serif;
  font-size: 0.65rem;
  font-weight: 700;
  background: #22c55e;
  color: #000;
  padding: 0 4px;
  border-radius: 3px;
}

.install-hint {
  display: block;
  font-size: 0.72rem;
  font-family: monospace;
  background: var(--surface);
  border: 1px solid var(--border);
  padding: 4px 8px;
  border-radius: 6px;
  color: var(--muted);
  white-space: pre;
  margin-top: 2px;
}

/* libavif section */
.avif-section {
  border-top: 1px solid var(--border);
  padding-top: 16px;
}

.avif-install-row {
  display: flex;
  align-items: center;
  gap: 12px;
  background: rgba(245, 158, 11, 0.06);
  border: 1px solid rgba(245, 158, 11, 0.2);
  border-radius: 8px;
  padding: 10px 14px;
  margin-top: 4px;
  flex-wrap: wrap;
}

.avif-install-note {
  font-size: 0.78rem;
  color: var(--muted);
  margin: 0;
  flex: 1;
}

.avif-install-note code {
  font-family: monospace;
  color: var(--text);
  background: var(--bg);
  padding: 1px 5px;
  border-radius: 3px;
}

.install-btn {
  flex-shrink: 0;
  padding: 7px 16px;
  background: #f59e0b;
  color: #000;
  border: none;
  border-radius: 6px;
  font-size: 0.82rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s;
}

.install-btn:hover { opacity: 0.85; }

.avif-error {
  width: 100%;
  font-size: 0.75rem;
  color: #ef4444;
  margin: 4px 0 0;
  word-break: break-all;
}

.avif-guide {
  margin-top: 6px;
  padding: 10px 14px;
  background: var(--bg);
  border-radius: 8px;
}

.guide-label {
  font-size: 0.78rem;
  color: var(--muted);
  margin: 0 0 6px;
}

.guide-commands {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.guide-commands code {
  font-family: monospace;
  font-size: 0.8rem;
  background: var(--surface);
  border: 1px solid var(--border);
  padding: 4px 10px;
  border-radius: 5px;
  color: var(--muted);
}

.guide-note {
  font-size: 0.76rem;
  color: var(--muted);
  margin: 6px 0 0;
  line-height: 1.5;
}
.guide-note a {
  color: var(--accent);
  text-decoration: none;
}
.guide-note a:hover { text-decoration: underline; }

.avif-custom-path {
  margin-top: 10px;
  padding: 10px 14px;
  background: var(--bg);
  border-radius: 8px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.path-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.path-label {
  font-size: 0.78rem;
  font-family: monospace;
  color: var(--muted);
  min-width: 60px;
}

.path-input {
  flex: 1;
  font-size: 0.78rem;
  font-family: monospace;
  background: var(--surface);
  border: 1px solid var(--border);
  color: var(--text);
  padding: 4px 8px;
  border-radius: 5px;
  outline: none;
}
.path-input:focus { border-color: var(--accent); }

.path-apply-btn {
  align-self: flex-start;
  font-size: 0.8rem;
  background: var(--accent);
  color: #fff;
  border: none;
  padding: 5px 14px;
  border-radius: 6px;
  cursor: pointer;
  transition: opacity 0.15s;
}
.path-apply-btn:disabled { opacity: 0.5; cursor: not-allowed; }
.path-apply-btn:hover:not(:disabled) { opacity: 0.85; }

.avif-installing {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 14px;
  background: var(--bg);
  border-radius: 8px;
}

.avif-install-label {
  font-size: 0.82rem;
  color: var(--text);
  margin: 0;
  font-weight: 600;
}

.bar-track {
  width: 100%;
  height: 6px;
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

.avif-log {
  max-height: 140px;
  overflow-y: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 0.7rem;
  color: var(--muted);
  line-height: 1.5;
}

.avif-log-line { white-space: pre-wrap; word-break: break-all; }

/* Full encoder/decoder list */
.full-codec-cols {
  display: flex;
  gap: 16px;
  margin-top: 10px;
}
.full-codec-col { flex: 1; min-width: 0; }
.full-codec-heading {
  font-size: 0.75rem;
  color: var(--muted);
  margin: 0 0 6px;
  font-weight: 600;
}
.full-codec-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}
.full-chip {
  font-size: 0.7rem;
  font-family: monospace;
  padding: 2px 6px;
  border-radius: 4px;
  border: 1px solid var(--border);
  color: var(--muted);
  background: var(--bg);
  white-space: nowrap;
}
.full-chip.hw-nvidia   { background: rgba(118,185,0,0.12);  color: #76b900; border-color: rgba(118,185,0,0.4); }
.full-chip.hw-amd      { background: rgba(237,28,36,0.1);   color: #ed4444; border-color: rgba(237,28,36,0.35); }
.full-chip.hw-intel    { background: rgba(0,150,255,0.1);   color: #0096ff; border-color: rgba(0,150,255,0.35); }
.full-chip.hw-apple    { background: rgba(160,160,160,0.1); color: #aaa;    border-color: rgba(160,160,160,0.35); }
.full-chip.hw-hw-other { background: rgba(100,200,100,0.1); color: #64c864; border-color: rgba(100,200,100,0.35); }
.hw-legend {
  display: flex;
  gap: 6px;
  margin-top: 10px;
  flex-wrap: wrap;
}

/* Build flags */
.build-flags {
  border-top: 1px solid var(--border);
  padding-top: 14px;
}

.build-flags summary {
  font-size: 0.8rem;
  color: var(--muted);
  cursor: pointer;
  user-select: none;
  margin-bottom: 10px;
}

.flags-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.flag-tag {
  font-size: 0.72rem;
  font-family: monospace;
  background: var(--bg);
  color: var(--muted);
  padding: 2px 6px;
  border-radius: 4px;
}

/* Spinner */
.center-msg {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 48px;
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

@keyframes spin { to { transform: rotate(360deg); } }
</style>
