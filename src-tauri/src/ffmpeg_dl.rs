use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri::ipc::Channel;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FfmpegStatus {
    pub available: bool,
    #[serde(rename = "hasWebp")]
    pub has_webp: bool,
    /// AVIF output possible at all (libsvtav1 or libaom-av1)
    #[serde(rename = "hasAvif")]
    pub has_avif: bool,
    /// AVIF output with alpha channel — requires libaom-av1 (svt-av1 drops alpha)
    #[serde(rename = "hasAvifAlpha")]
    pub has_avif_alpha: bool,
    pub path: String,
    #[serde(rename = "isManaged")]
    pub is_managed: bool,
    #[serde(rename = "downloadSupported")]
    pub download_supported: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadProgress {
    pub phase: String,
    pub percent: f64,
    pub message: String,
}

// ── Path helpers ──────────────────────────────────────────────────────────

pub fn managed_bin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|d| d.join("bin"))
        .map_err(|e| format!("app data dir unavailable: {e}"))
}

fn bin_name(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

pub fn managed_ffmpeg(app: &AppHandle) -> Option<PathBuf> {
    let p = managed_bin_dir(app).ok()?.join(bin_name("ffmpeg"));
    p.exists().then_some(p)
}

pub fn managed_ffprobe(app: &AppHandle) -> Option<PathBuf> {
    let p = managed_bin_dir(app).ok()?.join(bin_name("ffprobe"));
    p.exists().then_some(p)
}

pub fn resolve_ffmpeg(app: &AppHandle) -> String {
    if let Some(p) = managed_ffmpeg(app) { return p.to_string_lossy().into_owned(); }
    for p in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/usr/bin/ffmpeg"] {
        if Path::new(p).exists() { return p.to_string(); }
    }
    "ffmpeg".to_string()
}

pub fn resolve_ffprobe(app: &AppHandle) -> String {
    if let Some(p) = managed_ffprobe(app) { return p.to_string_lossy().into_owned(); }
    for p in ["/opt/homebrew/bin/ffprobe", "/usr/local/bin/ffprobe", "/usr/bin/ffprobe"] {
        if Path::new(p).exists() { return p.to_string(); }
    }
    "ffprobe".to_string()
}

// ── Status check ──────────────────────────────────────────────────────────

/// Whether an encoder actually exposes an alpha-capable pixel format.
/// (Encoder existence ≠ alpha support — e.g. ffmpeg's libaom-av1 has no yuva.)
async fn encoder_supports_alpha(path: &str, enc: &str) -> bool {
    let Ok(out) = Command::new(path)
        .args(["-hide_banner", "-h", &format!("encoder={enc}")])
        .output()
        .await
    else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.trim().strip_prefix("Supported pixel formats:"))
        .map(|s| {
            s.split_whitespace().any(|f| {
                f.contains("yuva")
                    || f.contains("rgba")
                    || f.contains("bgra")
                    || f.contains("argb")
                    || f.contains("abgr")
                    || f.starts_with("ya")
            })
        })
        .unwrap_or(false)
}

pub async fn check_status(app: &AppHandle) -> FfmpegStatus {
    let path = resolve_ffmpeg(app);
    let is_managed = managed_ffmpeg(app).is_some();

    let available = Command::new(&path)
        .arg("-version")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    // One -encoders call drives all capability flags.
    let encoders = if available {
        Command::new(&path)
            .args(["-hide_banner", "-encoders"])
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    } else {
        String::new()
    };

    let has_named = |name: &str| {
        encoders.lines().any(|l| {
            let mut cols = l.split_whitespace();
            cols.next(); // flags column
            cols.next() == Some(name)
        })
    };

    // WebP: external libwebp or built-in webp encoder
    let has_webp = encoders.contains("libwebp") || has_named("webp");
    // AVIF: any AV1 encoder produces AVIF stills
    let has_avif = has_named("libsvtav1") || has_named("libaom-av1") || has_named("librav1e");
    // Alpha-preserving AVIF: the encoder must actually expose an alpha pixel format.
    // libaom-av1 *exists* in ffmpeg but its wrapper does NOT publish yuva/rgba, so
    // ffmpeg silently drops alpha. Checking the encoder's pix_fmts is the only
    // reliable signal — presence of the encoder name is not enough.
    let mut has_avif_alpha = false;
    for enc in ["libaom-av1", "librav1e"] {
        if has_named(enc) && encoder_supports_alpha(&path, enc).await {
            has_avif_alpha = true;
            break;
        }
    }

    FfmpegStatus {
        available,
        has_webp,
        has_avif,
        has_avif_alpha,
        path,
        is_managed,
        download_supported: download_method() != DownloadMethod::Unsupported,
    }
}

// ── Download method selection ─────────────────────────────────────────────

#[derive(Debug, PartialEq)]
#[allow(dead_code)]
enum DownloadMethod {
    Brew,
    ZipDirect,
    Unsupported,
}

fn download_method() -> DownloadMethod {
    #[cfg(target_os = "macos")]      { return DownloadMethod::Brew; }
    #[cfg(target_os = "windows")]    { return DownloadMethod::ZipDirect; }
    #[allow(unreachable_code)]
    DownloadMethod::Unsupported
}

// ── macOS: install via Homebrew ───────────────────────────────────────────

#[cfg(target_os = "macos")]
fn find_brew() -> Result<String, String> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .iter()
        .find(|p| Path::new(p).exists())
        .map(|s| s.to_string())
        .ok_or_else(|| "Homebrew が見つかりません。https://brew.sh からインストールしてください。".to_string())
}

#[cfg(target_os = "macos")]
async fn run_brew(brew: &str, args: &[&str], on_progress: &Channel<DownloadProgress>, phase: &str, start_pct: f64) -> Result<(), String> {
    let mut child = Command::new(brew)
        .args(args)
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_EMOJI", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("brew 起動失敗: {e}"))?;

    let stderr = child.stderr.take().unwrap();
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    let mut tick = start_pct;

    while let Ok(Some(line)) = lines.next_line().await {
        let msg = line.trim().to_string();
        if msg.is_empty() { continue; }
        tick = (tick + 0.5).min(start_pct + 40.0);
        let _ = on_progress.send(DownloadProgress {
            phase: phase.to_string(),
            percent: tick,
            message: msg,
        });
    }

    let status = child.wait().await.map_err(|e| format!("待機エラー: {e}"))?;
    if !status.success() {
        return Err(format!("brew {} 失敗", args.join(" ")));
    }
    Ok(())
}

/// Whether the homebrew-ffmpeg tap formula is already installed.
#[cfg(target_os = "macos")]
async fn is_tap_ffmpeg_installed(brew: &str) -> bool {
    Command::new(brew)
        .args(["list", "homebrew-ffmpeg/ffmpeg/ffmpeg"])
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
async fn do_brew_install(on_progress: Channel<DownloadProgress>) -> Result<(), String> {
    let brew = find_brew()?;

    // ── Step 1: tap homebrew-ffmpeg/ffmpeg ────────────────────────────────
    let _ = on_progress.send(DownloadProgress {
        phase: "downloading_ffmpeg".to_string(),
        percent: 2.0,
        message: "brew tap homebrew-ffmpeg/ffmpeg ...".to_string(),
    });

    run_brew(&brew, &["tap", "homebrew-ffmpeg/ffmpeg"], &on_progress, "downloading_ffmpeg", 2.0).await?;

    // ── Step 2: install or reinstall with the codecs we need ──────────────
    // `install` is a no-op if the formula is already present with different
    // options, so when it's already installed we use `reinstall` to force a
    // rebuild with --with-webp --with-libvmaf (and the default libaom for AVIF alpha).
    let already = is_tap_ffmpeg_installed(&brew).await;

    // First-time install: remove the conflicting homebrew/core ffmpeg.
    // Both formulae provide the `ffmpeg` binary, so the core one must go first
    // or linking the tap formula fails. Errors (e.g. not installed) are ignored.
    if !already {
        let _ = on_progress.send(DownloadProgress {
            phase: "downloading_ffmpeg".to_string(),
            percent: 6.0,
            message: "通常版 ffmpeg を削除中（競合回避のため）...".to_string(),
        });
        let _ = run_brew(
            &brew,
            &["uninstall", "--ignore-dependencies", "--force", "ffmpeg"],
            &on_progress,
            "downloading_ffmpeg",
            6.0,
        ).await;
    }

    let action = if already { "reinstall" } else { "install" };
    let verb = if already { "再インストール" } else { "インストール" };

    let _ = on_progress.send(DownloadProgress {
        phase: "downloading_ffmpeg".to_string(),
        percent: 10.0,
        message: format!(
            "brew {action} homebrew-ffmpeg/ffmpeg/ffmpeg --with-webp\n\
             ソースからコンパイルのため 30〜60 分かかる場合があります（{verb}）..."
        ),
    });

    run_brew(
        &brew,
        &[action, "homebrew-ffmpeg/ffmpeg/ffmpeg", "--with-webp", "--with-libvmaf"],
        &on_progress,
        "downloading_ffmpeg",
        10.0,
    ).await?;

    // ── Verify ────────────────────────────────────────────────────────────
    let _ = on_progress.send(DownloadProgress {
        phase: "verifying".to_string(),
        percent: 95.0,
        message: "動作確認中...".to_string(),
    });

    let ffmpeg_path = if Path::new("/opt/homebrew/bin/ffmpeg").exists() {
        "/opt/homebrew/bin/ffmpeg"
    } else {
        "/usr/local/bin/ffmpeg"
    };

    match Command::new(ffmpeg_path).arg("-version").output().await {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let d = String::from_utf8_lossy(&o.stderr);
            return Err(format!("インストール後の確認失敗: {}", d.lines().next().unwrap_or("")));
        }
        Err(e) => return Err(format!("ffmpeg 起動失敗: {e}")),
    }

    Ok(())
}

// ── Windows: download zip from gyan.dev ──────────────────────────────────

#[cfg(target_os = "windows")]
async fn do_zip_install(app: &AppHandle, on_progress: Channel<DownloadProgress>) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("AnyCrunch/0.1")
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

    let _ = on_progress.send(DownloadProgress {
        phase: "downloading_ffmpeg".to_string(),
        percent: 0.0,
        message: "ffmpeg をダウンロード中...".to_string(),
    });

    let resp = client.get(url).send().await
        .map_err(|e| format!("ダウンロード失敗: {e}"))?;
    let total = resp.content_length();
    let mut buf = Vec::new();
    let mut response = resp;
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("受信エラー: {e}"))? {
        buf.extend_from_slice(&chunk);
        let pct = total.map(|t| (buf.len() as f64 / t as f64 * 90.0).min(89.0)).unwrap_or(0.0);
        let _ = on_progress.send(DownloadProgress {
            phase: "downloading_ffmpeg".to_string(),
            percent: pct,
            message: format!("ダウンロード中... ({:.0}%)", pct),
        });
    }

    if buf.len() < 4 || &buf[..2] != b"PK" {
        return Err("ZIP ではないデータを受信しました".to_string());
    }

    let _ = on_progress.send(DownloadProgress {
        phase: "extracting".to_string(),
        percent: 90.0,
        message: "展開中...".to_string(),
    });

    let bin_dir = managed_bin_dir(app)?;
    let ffmpeg_dest  = bin_dir.join("ffmpeg.exe");
    let ffprobe_dest = bin_dir.join("ffprobe.exe");
    extract_binary(&buf, "ffmpeg.exe",  &ffmpeg_dest)?;
    extract_binary(&buf, "ffprobe.exe", &ffprobe_dest)?;

    match Command::new(&ffmpeg_dest).arg("-version").output().await {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let d = String::from_utf8_lossy(&o.stderr);
            return Err(format!("動作確認失敗: {}", d.lines().next().unwrap_or("")));
        }
        Err(e) => return Err(format!("ffmpeg 起動失敗: {e}")),
    }

    Ok(())
}

// ── ZIP extraction (Windows) ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn extract_binary(zip_bytes: &[u8], target_name: &str, dest: &Path) -> Result<(), String> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("ZIP open: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("ZIP entry: {e}"))?;
        let file_name = entry.name().split(['/', '\\']).last().unwrap_or("").to_string();
        if file_name.to_lowercase() != target_name.to_lowercase() { continue; }
        if let Some(p) = dest.parent() { std::fs::create_dir_all(p).map_err(|e| format!("mkdir: {e}"))?; }
        let mut out = std::fs::File::create(dest).map_err(|e| format!("create: {e}"))?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| format!("read: {e}"))?;
        std::io::Write::write_all(&mut out, &buf).map_err(|e| format!("write: {e}"))?;
        return Ok(());
    }
    Err(format!("'{target_name}' が ZIP 内に見つかりません"))
}

// ── Public entry point ────────────────────────────────────────────────────

pub async fn do_download(
    _app: &AppHandle,
    on_progress: Channel<DownloadProgress>,
) -> Result<(), String> {
    match download_method() {
        #[cfg(target_os = "macos")]
        DownloadMethod::Brew => do_brew_install(on_progress).await,

        #[cfg(target_os = "windows")]
        DownloadMethod::ZipDirect => do_zip_install(_app, on_progress).await,

        DownloadMethod::Unsupported | _ => {
            Err("このプラットフォームは自動ダウンロードに対応していません".to_string())
        }
    }
}
