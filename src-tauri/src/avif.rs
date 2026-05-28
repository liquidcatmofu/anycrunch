#[allow(unused_imports)]
use std::process::Stdio;
use std::path::Path;
use serde::Serialize;
use tauri::ipc::Channel;
#[allow(unused_imports)]
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

// ── Path resolution ───────────────────────────────────────────────────────

pub fn resolve_avifenc(custom: Option<&str>) -> String {
    if let Some(p) = custom {
        if !p.is_empty() { return p.to_string(); }
    }
    #[cfg(not(target_os = "windows"))]
    for p in [
        "/opt/homebrew/bin/avifenc",
        "/usr/local/bin/avifenc",
        "/usr/bin/avifenc",
    ] {
        if Path::new(p).exists() { return p.to_string(); }
    }
    "avifenc".to_string()
}

pub fn resolve_avifdec(custom: Option<&str>) -> String {
    if let Some(p) = custom {
        if !p.is_empty() { return p.to_string(); }
    }
    #[cfg(not(target_os = "windows"))]
    for p in [
        "/opt/homebrew/bin/avifdec",
        "/usr/local/bin/avifdec",
        "/usr/bin/avifdec",
    ] {
        if Path::new(p).exists() { return p.to_string(); }
    }
    "avifdec".to_string()
}

// ── Status ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct AvifencStatus {
    pub available: bool,
    #[serde(rename = "avifdecAvailable")]
    pub avifdec_available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Whether automated install is supported on this platform.
    #[serde(rename = "installSupported")]
    pub install_supported: bool,
}

/// Returns true if the process starts (regardless of exit code).
async fn probe(binary: &str) -> bool {
    Command::new(binary)
        .arg("--help")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .is_ok()
}

async fn version_of(binary: &str) -> Option<String> {
    let out = Command::new(binary)
        .arg("--version")
        .output()
        .await
        .ok()?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    combined.lines().next().map(|l| l.trim().to_string())
}

pub async fn check_status(custom_enc: Option<&str>, custom_dec: Option<&str>) -> AvifencStatus {
    let enc_path = resolve_avifenc(custom_enc);
    let dec_path = resolve_avifdec(custom_dec);

    let available = probe(&enc_path).await;
    let avifdec_available = probe(&dec_path).await;
    let version = if available { version_of(&enc_path).await } else { None };

    AvifencStatus {
        available,
        avifdec_available,
        path: if available { Some(enc_path) } else { None },
        version,
        install_supported: cfg!(target_os = "macos"),
    }
}

// ── Install ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct AvifInstallProgress {
    pub phase: String,
    pub percent: f64,
    pub message: String,
}

#[cfg(target_os = "macos")]
fn find_brew() -> Result<String, String> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .iter()
        .find(|p| Path::new(p).exists())
        .map(|s| s.to_string())
        .ok_or_else(|| "Homebrew が見つかりません。https://brew.sh からインストールしてください。".to_string())
}

pub async fn do_install(on_progress: Channel<AvifInstallProgress>) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let brew = find_brew()?;

        let _ = on_progress.send(AvifInstallProgress {
            phase: "installing".to_string(),
            percent: 2.0,
            message: "brew install libavif ...".to_string(),
        });

        let mut child = Command::new(&brew)
            .args(["install", "libavif"])
            .env("HOMEBREW_NO_AUTO_UPDATE", "1")
            .env("HOMEBREW_NO_EMOJI", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("brew 起動失敗: {e}"))?;

        let stderr = child.stderr.take().unwrap();
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        let mut tick = 2.0f64;

        while let Ok(Some(line)) = lines.next_line().await {
            let msg = line.trim().to_string();
            if msg.is_empty() { continue; }
            tick = (tick + 0.8).min(88.0);
            let _ = on_progress.send(AvifInstallProgress {
                phase: "installing".to_string(),
                percent: tick,
                message: msg,
            });
        }

        let status = child.wait().await.map_err(|e| format!("待機エラー: {e}"))?;
        if !status.success() {
            return Err("brew install libavif 失敗".to_string());
        }

        let _ = on_progress.send(AvifInstallProgress {
            phase: "verifying".to_string(),
            percent: 95.0,
            message: "動作確認中...".to_string(),
        });

        if !probe(&resolve_avifenc(None)).await {
            return Err("インストール後の確認失敗: avifenc が見つかりません".to_string());
        }

        let _ = on_progress.send(AvifInstallProgress {
            phase: "done".to_string(),
            percent: 100.0,
            message: "インストール完了".to_string(),
        });

        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("このプラットフォームは自動インストールに対応していません".to_string())
}
