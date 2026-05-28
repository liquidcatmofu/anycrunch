# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**AnyCrunch** — ローカル完結でメディアを最適化・圧縮・変換するデスクトップ＋Webツール。外部サーバーへのアップロードは一切なし。MIT License.

## Development Commands

```bash
# フロントエンド (Vite)
pnpm dev           # Vite dev server のみ起動
pnpm build         # vue-tsc 型チェック + Vite プロダクションビルド
pnpm preview       # ビルド結果をプレビュー

# デスクトップ (Tauri)
pnpm tauri dev     # Tauri + Vite 開発モード
pnpm tauri build   # デスクトップリリースビルド

# Rust
cd src-tauri && cargo test        # Rustテスト
cd src-tauri && cargo clippy      # Rustリント
```

テストフレームワークは未セットアップ。追加後にコマンドを更新すること。

## Project Structure

```
src/                    # Vue 3 フロントエンド
  main.ts               # エントリポイント
  App.vue               # ルートコンポーネント
  assets/
src-tauri/              # Tauri / Rust バックエンド
  src/
    main.rs             # Tauri エントリポイント
    lib.rs              # Tauri コマンド定義
  Cargo.toml
```

## Architecture

### Processing Abstraction Layer

Vue UI は環境に依存しない `MediaProcessor` インターフェース経由で処理を呼ぶ。切り替えは `useProcessor` composable が担う：

```
Vue (UI)
  └─ MediaProcessor interface (TypeScript)
      ├─ WebProcessor    (WebCodecs / ffmpeg.wasm)
      └─ TauriProcessor  (Rust / FFmpeg sidecar)
```

```typescript
// useProcessor.ts
import { isTauri } from '@tauri-apps/api/core'

export function useProcessor(): MediaProcessor {
  if (isTauri()) return new TauriProcessor()
  return new WebProcessor()
}
```

### FFmpeg Integration

FFmpegは **サイドカー（別プロセス）** として実行する。Rustコード内へのリンク不可。**LGPL版FFmpegを使用すること**（libx264等を含まないビルド）。GPL版を同梱するとMITライセンスが汚染される。

FFmpeg取得フロー（初回使用時のみ）：
1. PATH検索 → 見つかれば即使用
2. 見つからない場合: 自動DL / パッケージマネージャ（winget/brew/apt） / 手動パス指定

## Core Type Definitions

実装の基準となる型定義（`src/types/` 等に配置予定）：

```typescript
interface MediaInfo {
  path: string
  type: 'image' | 'video' | 'audio' | 'archive' | 'unknown'
  size: number        // bytes
  mimeType: string
  width?: number; height?: number
  duration?: number   // seconds
  bitrate?: number    // bps
  codec?: string; fps?: number
}

type DiscordTier = 'free' | 'nitro_basic' | 'nitro'

type UseCase =
  | { type: 'discord';         tier: DiscordTier }
  | { type: 'discord_animate'; tier: DiscordTier }
  | { type: 'web' }
  | { type: 'storage' }
  | { type: 'archive'; priority: 'speed' | 'balanced' | 'size' }
  | { type: 'compat'; target: 'universal' | 'windows' | 'ios' }
  | { type: 'extract' }
  | { type: 'custom' }

interface TransformOptions {
  resize?: {
    width?: number; height?: number
    mode: 'fit' | 'fill' | 'stretch' | 'crop'
    anchor?: 'center' | 'top' | 'bottom' | 'left' | 'right'
  }
  aspectRatio?: {
    ratio: '16:9' | '4:3' | '1:1' | '9:16' | 'custom'
    customX?: number; customY?: number
  }
}

interface ProcessOptions {
  useCase: UseCase
  target?: { maxSizeBytes?: number; maxWidth?: number; maxHeight?: number; quality?: number }
  codec?: string; format?: string; hwAccel?: boolean
  transform?: TransformOptions
  description: string  // UI表示用
}

interface MediaProcessor {
  readonly platform: 'web' | 'desktop'
  analyze(file: File | string): Promise<MediaInfo>
  recommend(info: MediaInfo, useCase: UseCase, settings: AppSettings): Promise<RecommendResult>
  detectHwAccel(): Promise<string[]>
  process(input: File | string, options: ProcessOptions, onProgress: (p: ProcessProgress) => void): Promise<ProcessResult>
  cancel(fileId: string): Promise<void>
}

interface AppSettings {
  ffmpegPath?: string
  allowCpuAV1: boolean  // デフォルト: false
}
```

## Codec Selection Logic

HWエンコーダ優先順位：
- AV1: `nvenc_av1 > amf_av1 > qsv_av1`
- H.265: `nvenc_hevc > amf_hevc > qsv_hevc > videotoolbox_hevc`
- H.264: `nvenc_h264 > amf_h264 > qsv_h264 > videotoolbox_h264 > libx264`（fallback）

AV1ルール：
- HWエンコーダ未検出時はAV1を通常UIに表示しない
- `AppSettings.allowCpuAV1 = true` の場合のみ選択肢に表示
- CPU AV1選択時は必ず `strong` 警告を出す

## Design Decisions

- **Discord制限値はハードコードせず設定ファイルで管理**（サービス変更対応）
- **GIFはデフォルト非推奨**：アニメWebPを優先、GIF選択時は `caution` 警告
- **UseCase と TransformOptions は直交**：プリセット選択後に変形オプションを追加する
- **見積もり精度は初期リリースでは粗い推定＋「目安」注釈**で許容
- **Discordプリセット選択時のみ** tier選択サブUIを表示
- **中間形式（ProRes/TIFF等）はv2対応**、v1スコープ外
- **Safari非対応**：使用時に誘導メッセージを表示。PWA・スマホアプリもスコープ外

## Web Version Constraints

- 対応ブラウザ: Chromium系 / Firefox系のみ
- 音声: Web Audio API経由の軽い処理のみ
- 動画: ffmpeg.wasm使用、`strong` 警告（「デスクトップ版推奨」）を表示
- ffprobe/FFmpegはブラウザから呼べないため Web版の解析は別実装が必要

## Windows Compatibility Announcement

HEIC / AVIF検出時、Windowsのみ表示：
- 変換する（JPEG/PNGへ）
- Microsoft Storeで拡張を入手
  - HEIC: `ms-windows-store://pdp/?ProductId=9pmmsr1cgpwg`
  - AVIF: `ms-windows-store://pdp/?ProductId=9mvzqvxjbq9v`
- 無視する
