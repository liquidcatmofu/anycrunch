mod avif;
mod ffmpeg_dl;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use ffmpeg_dl::{resolve_ffmpeg, resolve_ffprobe};

// ── App state ─────────────────────────────────────────────────────────────

pub struct ProcessRegistry(pub Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>>);

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MediaInfo {
    pub path: String,
    #[serde(rename = "type")]
    pub media_type: String,
    pub size: u64,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<f64>,
    pub bitrate: Option<u64>,
    pub codec: Option<String>,
    pub fps: Option<f64>,
    #[serde(rename = "hasAlpha")]
    pub has_alpha: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcessProgress {
    #[serde(rename = "fileId")]
    pub file_id: String,
    pub percent: f64,
    pub eta: Option<f64>,
    #[serde(rename = "currentStep")]
    pub current_step: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcessResult {
    pub success: bool,
    #[serde(rename = "outputPath")]
    pub output_path: String,
    #[serde(rename = "originalSize")]
    pub original_size: u64,
    #[serde(rename = "outputSize")]
    pub output_size: u64,
    pub duration: u64,
    pub error: Option<String>,
}

// ── ffprobe deserialization ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: FfprobeFormat,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    r_frame_rate: Option<String>,
    pix_fmt: Option<String>,
    #[allow(dead_code)]
    bit_rate: Option<String>,
    #[allow(dead_code)]
    duration: Option<String>,
}

/// Detect alpha channel from ffprobe's pix_fmt.
/// Only flag pixel formats that definitively carry an alpha component,
/// to avoid false positives on opaque palettized images.
fn pix_fmt_has_alpha(pix_fmt: &str) -> bool {
    const ALPHA_FMTS: &[&str] = &[
        "rgba", "bgra", "argb", "abgr",
        "yuva420p", "yuva422p", "yuva444p",
        "ya8", "ya16",
        "gbrap", "gbrap16le", "gbrap16be",
        "rgba64le", "rgba64be", "bgra64le", "bgra64be",
    ];
    let f = pix_fmt.to_lowercase();
    ALPHA_FMTS.iter().any(|a| f == *a)
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    #[allow(dead_code)]
    filename: String,
    size: Option<String>,
    duration: Option<String>,
    bit_rate: Option<String>,
    format_name: Option<String>,
}

// ── ProcessOptions from TypeScript ────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TargetOptions {
    #[serde(rename = "maxSizeBytes")]
    max_size_bytes: Option<u64>,
    #[allow(dead_code)]
    #[serde(rename = "maxWidth")]
    max_width: Option<u32>,
    #[allow(dead_code)]
    #[serde(rename = "maxHeight")]
    max_height: Option<u32>,
    quality: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ResizeOptions {
    width: Option<u32>,
    height: Option<u32>,
    #[allow(dead_code)]
    mode: String,
}

#[derive(Debug, Deserialize)]
struct TransformOptions {
    resize: Option<ResizeOptions>,
}

#[derive(Debug, Deserialize)]
struct ProcessOptionsJs {
    #[serde(rename = "useCase")]
    use_case: serde_json::Value,
    target: Option<TargetOptions>,
    codec: Option<String>,
    format: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "hwAccel")]
    hw_accel: Option<bool>,
    transform: Option<TransformOptions>,
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn parse_fps(r_frame_rate: &str) -> Option<f64> {
    let parts: Vec<&str> = r_frame_rate.split('/').collect();
    if parts.len() == 2 {
        let num: f64 = parts[0].parse().ok()?;
        let den: f64 = parts[1].parse().ok()?;
        if den == 0.0 { return None; }
        Some((num / den * 100.0).round() / 100.0)
    } else {
        r_frame_rate.parse().ok()
    }
}

fn media_type_from_probe(streams: &[FfprobeStream], format_name: &str, ext: &str) -> String {
    // Extension-based override for container-based still image formats.
    // AVIF uses AV1 codec, HEIC/HEIF use HEVC — both look like video to ffprobe.
    let still_image_exts = ["avif", "heic", "heif"];
    if still_image_exts.contains(&ext) {
        return "image".to_string();
    }

    let image_formats = ["image2", "png_pipe", "jpeg_pipe", "webp_pipe", "gif"];
    if image_formats.iter().any(|f| format_name.contains(f)) {
        return "image".to_string();
    }

    let has_video = streams.iter().any(|s| s.codec_type.as_deref() == Some("video"));
    let has_audio = streams.iter().any(|s| s.codec_type.as_deref() == Some("audio"));
    if has_video {
        let image_codecs = ["mjpeg", "png", "bmp", "tiff", "webp"];
        let vc = streams.iter()
            .find(|s| s.codec_type.as_deref() == Some("video"))
            .and_then(|s| s.codec_name.as_deref())
            .unwrap_or("");
        if image_codecs.contains(&vc) { return "image".to_string(); }
        "video".to_string()
    } else if has_audio {
        "audio".to_string()
    } else {
        "unknown".to_string()
    }
}

fn mime_type_for(path: &str, media_type: &str) -> String {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png"          => "image/png",
        "webp"         => "image/webp",
        "avif"         => "image/avif",
        "heic" | "heif"=> "image/heic",
        "gif"          => "image/gif",
        "mp4" | "m4v"  => "video/mp4",
        "webm"         => "video/webm",
        "mov"          => "video/quicktime",
        "mkv"          => "video/x-matroska",
        "avi"          => "video/x-msvideo",
        "mp3"          => "audio/mpeg",
        "aac"          => "audio/aac",
        "opus"         => "audio/opus",
        "flac"         => "audio/flac",
        "wav"          => "audio/wav",
        "m4a"          => "audio/mp4",
        "ogg"          => "audio/ogg",
        "zip"          => "application/zip",
        "7z"           => "application/x-7z-compressed",
        "tar"          => "application/x-tar",
        "gz"           => "application/gzip",
        "bz2"          => "application/x-bzip2",
        "rar"          => "application/x-rar-compressed",
        _ => match media_type {
            "video"   => "video/mp4",
            "audio"   => "audio/mpeg",
            "image"   => "image/jpeg",
            _         => "application/octet-stream",
        },
    }.to_string()
}

fn is_archive(path: &str) -> bool {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    matches!(ext.as_str(), "zip" | "7z" | "tar" | "gz" | "bz2" | "rar" | "xz" | "zst")
}

fn output_path(input: &str, format: Option<&str>) -> String {
    let p = Path::new(input);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = format.unwrap_or_else(|| p.extension().and_then(|e| e.to_str()).unwrap_or("out"));
    let parent = p.parent().unwrap_or(Path::new("."));
    parent.join(format!("{}_anycrunch.{}", stem, ext)).to_string_lossy().to_string()
}

fn preview_cache_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("previews")
}

fn preview_output_path(app: &AppHandle, preview_id: &str, format: &str) -> String {
    preview_cache_dir(app)
        .join(format!("{}.{}", preview_id, format))
        .to_string_lossy()
        .to_string()
}

// ── AVIF helpers ──────────────────────────────────────────────────────────

fn tmp_png_path(input: &str, suffix: &str) -> String {
    let p = Path::new(input);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("tmp");
    let parent = p.parent().unwrap_or(Path::new("."));
    parent.join(format!("{}_{}.png", stem, suffix)).to_string_lossy().to_string()
}

/// Decode AVIF → PNG using avifdec (preserves alpha).
async fn run_avifdec(input: &str, output: &str) -> Result<(), String> {
    let dec = avif::resolve_avifdec();
    let out = Command::new(&dec)
        .args([input, output])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("avifdec 起動失敗: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("avifdec 失敗: {}", msg.lines().last().unwrap_or("unknown")));
    }
    Ok(())
}

/// Decode input → PNG using ffmpeg (for non-AVIF formats).
async fn run_ffmpeg_to_png(
    ffmpeg: &str,
    input: &str,
    output: &str,
    resize: Option<&ResizeOptions>,
) -> Result<(), String> {
    let mut args = vec![
        "-i".to_string(), input.to_string(),
        "-y".to_string(),
        "-frames:v".to_string(), "1".to_string(),
    ];
    if let Some(r) = resize {
        let scale = match (r.width, r.height) {
            (Some(w), Some(h)) => format!("scale={}:{}", w, h),
            (Some(w), None)    => format!("scale={}:-2", w),
            (None, Some(h))    => format!("scale=-2:{}", h),
            _                  => String::new(),
        };
        if !scale.is_empty() {
            args.extend(["-vf".to_string(), scale]);
        }
    }
    args.push(output.to_string());
    let out = Command::new(ffmpeg)
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("ffmpeg PNG 変換失敗: {e}"))?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).to_string();
        return Err(format!("ffmpeg PNG 変換失敗: {}", msg.lines().last().unwrap_or("unknown")));
    }
    Ok(())
}

// ── ffmpeg arg builders ────────────────────────────────────────────────────

fn build_video_args(opts: &ProcessOptionsJs, input: &str) -> (Vec<String>, String) {
    let use_case_type = opts.use_case.get("type").and_then(|v| v.as_str()).unwrap_or("custom");
    let quality = opts.target.as_ref().and_then(|t| t.quality).unwrap_or(23);
    let format = opts.format.as_deref().unwrap_or("mp4");

    let video_codec = match opts.codec.as_deref() {
        Some(c) => c.to_string(),
        None => match use_case_type {
            "storage" => "libx265".to_string(),
            _         => "libx264".to_string(),
        },
    };

    let out = output_path(input, Some(format));
    let mut args = vec!["-i".to_string(), input.to_string(), "-y".to_string()];

    args.extend(["-c:v".to_string(), video_codec.clone()]);

    let has_size_target = opts.target.as_ref().and_then(|t| t.max_size_bytes).is_some();
    if video_codec.contains("x264") || video_codec.contains("x265") {
        if has_size_target {
            // Size-target mode: use ABR, not CRF.
            // Emitting both -crf and -b:v simultaneously causes libx264/libx265 to
            // run ABR with CRF as a quality ceiling — the size target still works
            // roughly, but skipping -crf gives cleaner ABR behaviour.
        } else {
            args.extend(["-crf".to_string(), quality.to_string()]);
            let preset = if video_codec.contains("x265") { "medium" } else { "fast" };
            args.extend(["-preset".to_string(), preset.to_string()]);
        }
    }
    args.extend(["-c:a".to_string(), "aac".to_string(), "-b:a".to_string(), "128k".to_string()]);

    if let Some(transform) = &opts.transform {
        if let Some(resize) = &transform.resize {
            let scale = match (resize.width, resize.height) {
                (Some(w), Some(h)) => format!("scale={}:{}", w, h),
                (Some(w), None)    => format!("scale={}:-2", w),
                (None, Some(h))    => format!("scale=-2:{}", h),
                _                  => String::new(),
            };
            if !scale.is_empty() { args.extend(["-vf".to_string(), scale]); }
        }
    }

    if let Some(max_bytes) = opts.target.as_ref().and_then(|t| t.max_size_bytes) {
        let video_kbps = ((max_bytes as f64 * 8.0 * 0.9) / 1000.0) as u64;
        let video_kbps = video_kbps.saturating_sub(128);
        if video_kbps > 0 { args.extend(["-b:v".to_string(), format!("{}k", video_kbps)]); }
    }

    args.extend(["-progress".to_string(), "pipe:2".to_string(), "-nostats".to_string()]);
    args.push(out.clone());
    (args, out)
}

fn build_image_args(opts: &ProcessOptionsJs, input: &str) -> (Vec<String>, String) {
    let quality = opts.target.as_ref().and_then(|t| t.quality).unwrap_or(80);
    // Default to JPEG — always available in any ffmpeg build.
    // WebP/AVIF require external libs and are only used when explicitly selected.
    let format = opts.format.as_deref().unwrap_or("jpg");

    let out = output_path(input, Some(format));
    let mut args = vec!["-i".to_string(), input.to_string(), "-y".to_string()];

    match format {
        "webp" => {
            // Don't force -c:v libwebp; let ffmpeg auto-select the available WebP encoder
            // (libwebp external or built-in webp). -quality works for libwebp;
            // for the built-in encoder ffmpeg uses its own defaults.
            args.extend([
                "-quality".to_string(), quality.to_string(),
            ]);
        }
        "avif" => {
            // SVT-AV1 is available on Homebrew ffmpeg; libaom-av1 typically is not.
            // For AVIF still images, SVT-AV1 + -crf works correctly.
            args.extend([
                "-c:v".to_string(), "libsvtav1".to_string(),
                "-crf".to_string(), ((100 - quality) / 2).to_string(),
            ]);
        }
        "jpg" | "jpeg" => {
            let q_scale = ((100 - quality) / 5 + 2).clamp(2, 31);
            args.extend([
                "-c:v".to_string(), "mjpeg".to_string(),
                "-q:v".to_string(), q_scale.to_string(),
                // mjpeg has no alpha; force a non-alpha pixel format so converting
                // a transparent PNG/WebP doesn't fail (transparency is dropped).
                "-pix_fmt".to_string(), "yuvj420p".to_string(),
            ]);
        }
        _ => {}
    }

    if let Some(transform) = &opts.transform {
        if let Some(resize) = &transform.resize {
            let scale = match (resize.width, resize.height) {
                (Some(w), Some(h)) => format!("scale={}:{}", w, h),
                (Some(w), None)    => format!("scale={}:-2", w),
                (None, Some(h))    => format!("scale=-2:{}", h),
                _                  => String::new(),
            };
            if !scale.is_empty() && !args.contains(&"-vf".to_string()) {
                args.extend(["-vf".to_string(), scale]);
            }
        }
    }

    args.push(out.clone());
    (args, out)
}

fn build_audio_args(opts: &ProcessOptionsJs, input: &str) -> (Vec<String>, String) {
    let format = opts.format.as_deref().unwrap_or("opus");
    let out = output_path(input, Some(format));

    // Quality preset → bitrate: target.quality is kbps when set by the new UI.
    // Falls back to use-case heuristics for backward compatibility.
    let bitrate = if let Some(q) = opts.target.as_ref().and_then(|t| t.quality) {
        format!("{}k", q)
    } else {
        let use_case_type = opts.use_case.get("type").and_then(|v| v.as_str()).unwrap_or("custom");
        match use_case_type {
            "discord" => match opts.use_case.get("tier").and_then(|v| v.as_str()).unwrap_or("free") {
                "nitro_basic" => "128k",
                "nitro"       => "192k",
                _             => "96k",
            }.to_string(),
            "web"     => "128k".to_string(),
            "storage" => "160k".to_string(),
            _         => "128k".to_string(),
        }
    };

    let audio_codec = match format {
        "opus" | "ogg" => "libopus",
        "aac" | "m4a"  => "aac",
        "mp3"          => "libmp3lame",
        _              => "libopus",
    };

    let mut args = vec!["-i".to_string(), input.to_string(), "-y".to_string()];
    args.extend([
        "-c:a".to_string(), audio_codec.to_string(),
        "-b:a".to_string(), bitrate.to_string(),
        "-vn".to_string(),
    ]);
    args.push(out.clone());
    (args, out)
}

fn parse_progress_time(line: &str) -> Option<f64> {
    if let Some(rest) = line.strip_prefix("out_time_us=") {
        let us: f64 = rest.trim().parse().ok()?;
        return Some(us / 1_000_000.0);
    }
    if let Some(rest) = line.strip_prefix("time=") {
        let parts: Vec<&str> = rest.trim().split(':').collect();
        if parts.len() == 3 {
            let h: f64 = parts[0].parse().ok()?;
            let m: f64 = parts[1].parse().ok()?;
            let s: f64 = parts[2].parse().ok()?;
            return Some(h * 3600.0 + m * 60.0 + s);
        }
    }
    None
}

// ── Core logic (non-command) ───────────────────────────────────────────────

async fn do_analyze(path: &str, app: &AppHandle) -> Result<MediaInfo, String> {
    if is_archive(path) {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        return Ok(MediaInfo {
            path: path.to_string(),
            media_type: "archive".to_string(),
            size,
            mime_type: mime_type_for(path, "archive"),
            width: None, height: None, duration: None,
            bitrate: None, codec: None, fps: None, has_alpha: None,
        });
    }

    let output = Command::new(resolve_ffprobe(app))
        .args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams", path])
        .output()
        .await
        .map_err(|e| format!("ffprobe failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("ffprobe error: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let probe: FfprobeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("ffprobe parse error: {}", e))?;

    let format_name = probe.format.format_name.as_deref().unwrap_or("");
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let media_type  = media_type_from_probe(&probe.streams, format_name, &ext);
    let size = probe.format.size.as_deref()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0));
    let duration = probe.format.duration.as_deref().and_then(|s| s.parse().ok());
    let bitrate  = probe.format.bit_rate.as_deref().and_then(|s| s.parse::<u64>().ok());

    let video_stream = probe.streams.iter().find(|s| s.codec_type.as_deref() == Some("video"));
    let (width, height, codec, fps, has_alpha) = if let Some(vs) = video_stream {
        let fps = vs.r_frame_rate.as_deref().and_then(parse_fps);
        // Alpha only meaningful for images; videos rarely carry usable alpha
        let alpha = if media_type == "image" {
            Some(vs.pix_fmt.as_deref().map(pix_fmt_has_alpha).unwrap_or(false))
        } else {
            None
        };
        (vs.width, vs.height, vs.codec_name.clone(), fps, alpha)
    } else {
        let audio = probe.streams.iter().find(|s| s.codec_type.as_deref() == Some("audio"));
        (None, None, audio.and_then(|s| s.codec_name.clone()), None, None)
    };

    Ok(MediaInfo {
        path: path.to_string(),
        mime_type: mime_type_for(path, &media_type),
        media_type, size, duration, bitrate, codec, fps, width, height, has_alpha,
    })
}

// ── Tauri commands ────────────────────────────────────────────────────────

#[tauri::command]
async fn analyze_file(path: String, app: AppHandle) -> Result<MediaInfo, String> {
    do_analyze(&path, &app).await
}

#[tauri::command]
async fn detect_hw_accel(app: AppHandle) -> Result<Vec<String>, String> {
    let output = Command::new(resolve_ffmpeg(&app))
        .args(["-hide_banner", "-encoders"])
        .output()
        .await
        .map_err(|e| format!("ffmpeg failed: {}", e))?;

    let text = String::from_utf8_lossy(&output.stdout).to_string()
        + &String::from_utf8_lossy(&output.stderr);

    let hw_encoders = [
        ("av1_nvenc",         "nvenc_av1"),
        ("av1_amf",           "amf_av1"),
        ("av1_qsv",           "qsv_av1"),
        ("hevc_nvenc",        "nvenc_hevc"),
        ("hevc_amf",          "amf_hevc"),
        ("hevc_qsv",          "qsv_hevc"),
        ("hevc_videotoolbox", "videotoolbox_hevc"),
        ("h264_nvenc",        "nvenc_h264"),
        ("h264_amf",          "amf_h264"),
        ("h264_qsv",          "qsv_h264"),
        ("h264_videotoolbox", "videotoolbox_h264"),
    ];

    Ok(hw_encoders.iter()
        .filter(|(name, _)| text.contains(name))
        .map(|(_, id)| id.to_string())
        .collect())
}

#[tauri::command]
async fn process_file(
    file_id: String,
    input: String,
    options: serde_json::Value,
    on_progress: Channel<ProcessProgress>,
    registry: State<'_, ProcessRegistry>,
    app: AppHandle,
) -> Result<ProcessResult, String> {
    let opts: ProcessOptionsJs = serde_json::from_value(options)
        .map_err(|e| format!("Invalid options: {}", e))?;

    let original_size = std::fs::metadata(&input).map(|m| m.len()).unwrap_or(0);
    let start = Instant::now();

    let ext = Path::new(&input).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let media_type = if is_archive(&input) {
        "archive"
    } else {
        match ext.as_str() {
            "jpg"|"jpeg"|"png"|"webp"|"avif"|"heic"|"heif"|"gif"|"bmp"|"tiff" => "image",
            "mp3"|"aac"|"opus"|"flac"|"wav"|"ogg"|"m4a" => "audio",
            _ => "video",
        }
    };

    let output_fmt = opts.format.as_deref().unwrap_or("").to_string();

    // ── AVIF output via avifenc ───────────────────────────────────────────
    // avifenc handles alpha natively; FFmpeg's libsvtav1 does not.
    if media_type == "image" && output_fmt == "avif" {
        let avif_status = avif::check_status().await;
        if avif_status.available {
            let quality_input = opts.target.as_ref().and_then(|t| t.quality).unwrap_or(80);
            // avifenc quality scale: 100 = lossless, 0 = worst.
            // Our quality input uses a JPEG-like 0-100 scale where 80 = typical web quality.
            // Passing 80 directly to avifenc produces near-lossless output (much larger than JPEG 80).
            // Mapping: avif_q ≈ input_quality * 0.65 gives perceptually comparable results.
            let quality = ((quality_input as f64 * 0.65) as u32).clamp(1, 95);
            let out_path = output_path(&input, Some("avif"));
            let resize = opts.transform.as_ref().and_then(|t| t.resize.as_ref());

            // Decode source to lossless PNG first.
            // AVIF inputs: use avifdec (FFmpeg may drop alpha).
            // All other inputs: use FFmpeg (handles WebP, HEIC, JPEG, etc.).
            let tmp_png = tmp_png_path(&input, "enc_tmp");
            let decode_result = if ext == "avif" && avif_status.avifdec_available {
                run_avifdec(&input, &tmp_png).await
            } else {
                run_ffmpeg_to_png(&resolve_ffmpeg(&app), &input, &tmp_png, resize).await
            };
            if let Err(e) = decode_result {
                return Ok(ProcessResult {
                    success: false, output_path: String::new(),
                    original_size, output_size: 0,
                    duration: start.elapsed().as_millis() as u64,
                    error: Some(e),
                });
            }

            let _ = on_progress.send(ProcessProgress {
                file_id: file_id.clone(), percent: 30.0,
                eta: None, current_step: "encoding".to_string(),
            });

            let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
            registry.0.lock().unwrap().insert(file_id.clone(), kill_tx);

            let quality_str = quality.to_string();
            let enc = avif::resolve_avifenc();
            let mut child = match Command::new(&enc)
                .args([&tmp_png, "-q", &quality_str, "-j", "8", "-o", &out_path])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    registry.0.lock().unwrap().remove(&file_id);
                    let _ = std::fs::remove_file(&tmp_png);
                    return Err(format!("avifenc 起動失敗: {e}"));
                }
            };

            let enc_stderr = child.stderr.take().expect("stderr");
            let mut enc_lines = tokio::io::BufReader::new(enc_stderr).lines();
            let mut enc_stderr_log: Vec<String> = Vec::new();

            loop {
                tokio::select! {
                    line = enc_lines.next_line() => match line {
                        Ok(Some(l)) => enc_stderr_log.push(l),
                        _           => break,
                    },
                    _ = &mut kill_rx => {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        registry.0.lock().unwrap().remove(&file_id);
                        let _ = std::fs::remove_file(&tmp_png);
                        return Err("Cancelled".to_string());
                    }
                }
            }

            let enc_status = child.wait().await.map_err(|e| format!("Wait error: {e}"))?;
            registry.0.lock().unwrap().remove(&file_id);
            let _ = std::fs::remove_file(&tmp_png);

            if !enc_status.success() {
                let error_msg = enc_stderr_log.iter().rev()
                    .find(|l| !l.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| "avifenc 失敗".to_string());
                return Ok(ProcessResult {
                    success: false, output_path: String::new(),
                    original_size, output_size: 0,
                    duration: start.elapsed().as_millis() as u64,
                    error: Some(error_msg),
                });
            }

            let output_size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            if output_size == 0 {
                return Ok(ProcessResult {
                    success: false, output_path: out_path,
                    original_size, output_size: 0,
                    duration: start.elapsed().as_millis() as u64,
                    error: Some("出力ファイルが生成されませんでした".to_string()),
                });
            }

            let _ = on_progress.send(ProcessProgress {
                file_id: file_id.clone(), percent: 100.0,
                eta: Some(0.0), current_step: "done".to_string(),
            });
            return Ok(ProcessResult {
                success: true, output_path: out_path,
                original_size, output_size,
                duration: start.elapsed().as_millis() as u64,
                error: None,
            });
        }
        // avifenc not available: fall through to FFmpeg path (no alpha support)
    }

    // ── AVIF input decode via avifdec (preserves alpha for non-AVIF output) ─
    let (actual_input, tmp_avif_png) = if ext == "avif" && output_fmt != "avif" {
        let avif_status = avif::check_status().await;
        if avif_status.avifdec_available {
            let tmp = tmp_png_path(&input, "dec_tmp");
            match run_avifdec(&input, &tmp).await {
                Ok(_) => (tmp.clone(), Some(tmp)),
                Err(_) => (input.clone(), None), // fall back to direct FFmpeg
            }
        } else {
            (input.clone(), None)
        }
    } else {
        (input.clone(), None)
    };

    let (args, out_path) = match media_type {
        "image" => build_image_args(&opts, &actual_input),
        "audio" => build_audio_args(&opts, &actual_input),
        _       => build_video_args(&opts, &actual_input),
    };

    let total_duration: Option<f64> = if media_type == "video" || media_type == "audio" {
        do_analyze(&actual_input, &app).await.ok().and_then(|i| i.duration)
    } else {
        None
    };

    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    registry.0.lock().unwrap().insert(file_id.clone(), kill_tx);

    let mut child = match Command::new(resolve_ffmpeg(&app))
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            if let Some(ref tmp) = tmp_avif_png { let _ = std::fs::remove_file(tmp); }
            return Err(format!("Failed to spawn ffmpeg: {}", e));
        }
    };

    let stderr = child.stderr.take().expect("stderr not captured");
    let mut lines = tokio::io::BufReader::new(stderr).lines();
    let mut stderr_log: Vec<String> = Vec::new();

    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(l)) => {
                        stderr_log.push(l.clone());
                        if stderr_log.len() > 60 { stderr_log.remove(0); }

                        if let Some(elapsed) = parse_progress_time(&l) {
                            let percent = total_duration
                                .map(|t| (elapsed / t * 100.0).clamp(0.0, 99.0))
                                .unwrap_or(50.0);
                            let _ = on_progress.send(ProcessProgress {
                                file_id: file_id.clone(),
                                percent,
                                eta: total_duration.map(|t| (t - elapsed).max(0.0)),
                                current_step: "encoding".to_string(),
                            });
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
            _ = &mut kill_rx => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                registry.0.lock().unwrap().remove(&file_id);
                if let Some(ref tmp) = tmp_avif_png { let _ = std::fs::remove_file(tmp); }
                return Err("Cancelled".to_string());
            }
        }
    }

    let status = child.wait().await.map_err(|e| format!("Wait error: {}", e))?;
    registry.0.lock().unwrap().remove(&file_id);
    if let Some(ref tmp) = tmp_avif_png { let _ = std::fs::remove_file(tmp); }

    if !status.success() {
        let error_msg = stderr_log.iter().rev()
            .find(|l| {
                let l = l.to_lowercase();
                l.starts_with("error") || l.contains("no such file") || l.contains("invalid")
                    || l.contains("unknown encoder") || l.contains("encoder") && l.contains("not found")
                    || l.contains("codec not found")
            })
            .or_else(|| stderr_log.last())
            .cloned()
            .unwrap_or_else(|| "ffmpeg failed (unknown error)".to_string());

        return Ok(ProcessResult {
            success: false,
            output_path: String::new(),
            original_size,
            output_size: 0,
            duration: start.elapsed().as_millis() as u64,
            error: Some(error_msg),
        });
    }

    let output_size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    if output_size == 0 {
        return Ok(ProcessResult {
            success: false,
            output_path: out_path,
            original_size,
            output_size: 0,
            duration: start.elapsed().as_millis() as u64,
            error: Some("出力ファイルが生成されませんでした".to_string()),
        });
    }

    let _ = on_progress.send(ProcessProgress {
        file_id: file_id.clone(),
        percent: 100.0,
        eta: Some(0.0),
        current_step: "done".to_string(),
    });

    Ok(ProcessResult {
        success: true,
        output_path: out_path,
        original_size,
        output_size,
        duration: start.elapsed().as_millis() as u64,
        error: None,
    })
}

#[tauri::command]
async fn cancel_process(
    file_id: String,
    registry: State<'_, ProcessRegistry>,
) -> Result<(), String> {
    if let Some(tx) = registry.0.lock().unwrap().remove(&file_id) {
        let _ = tx.send(());
    }
    Ok(())
}

/// Copy an already-processed preview temp file to the final output destination.
/// This skips re-encoding when a valid preview already exists.
#[tauri::command]
async fn save_from_preview(
    preview_path: String,
    input_path: String,
    format: String,
) -> Result<ProcessResult, String> {
    let original_size = std::fs::metadata(&input_path).map(|m| m.len()).unwrap_or(0);
    let out_path = output_path(&input_path, Some(&format));

    std::fs::copy(&preview_path, &out_path)
        .map_err(|e| format!("保存失敗: {e}"))?;

    // Clean up the preview temp file after saving
    let _ = std::fs::remove_file(&preview_path);

    let output_size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    Ok(ProcessResult {
        success: true,
        output_path: out_path,
        original_size,
        output_size,
        duration: 0,
        error: None,
    })
}

// ── avifenc commands ─────────────────────────────────────────────────────

#[tauri::command]
async fn check_avifenc() -> avif::AvifencStatus {
    avif::check_status().await
}

#[tauri::command]
async fn install_avifenc(
    on_progress: Channel<avif::AvifInstallProgress>,
) -> Result<(), String> {
    avif::do_install(on_progress).await
}

// ── Clipboard commands ────────────────────────────────────────────────────

// ── Preview commands ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct PreviewResult {
    path: String,
    #[serde(rename = "outputSize")]
    output_size: u64,
}

/// Process an image to a temp cache path for live preview.
/// Re-uses the same AVIF/FFmpeg routing as process_file but without
/// progress reporting and with an explicit cache output path.
#[tauri::command]
async fn preview_image(
    preview_id: String,
    input: String,
    options: serde_json::Value,
    registry: State<'_, ProcessRegistry>,
    app: AppHandle,
) -> Result<PreviewResult, String> {
    let opts: ProcessOptionsJs = serde_json::from_value(options)
        .map_err(|e| format!("Invalid options: {}", e))?;

    let ext = Path::new(&input).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let output_fmt = opts.format.as_deref().unwrap_or("jpg").to_string();
    let out = preview_output_path(&app, &preview_id, &output_fmt);

    let dir = preview_cache_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cache dir: {e}"))?;

    let (kill_tx, mut kill_rx) = tokio::sync::oneshot::channel::<()>();
    registry.0.lock().unwrap().insert(preview_id.clone(), kill_tx);

    macro_rules! check_cancel {
        () => {
            if kill_rx.try_recv().is_ok() {
                registry.0.lock().unwrap().remove(&preview_id);
                let _ = std::fs::remove_file(&out);
                return Err("Cancelled".to_string());
            }
        };
    }

    // ── AVIF output via avifenc ──────────────────────────────────────────
    if output_fmt == "avif" {
        let avif_status = avif::check_status().await;
        if avif_status.available {
            let quality_input = opts.target.as_ref().and_then(|t| t.quality).unwrap_or(80);
            let quality = ((quality_input as f64 * 0.65) as u32).clamp(1, 95);
            let resize = opts.transform.as_ref().and_then(|t| t.resize.as_ref());
            let tmp_png = format!("{}.enc.png", out);

            check_cancel!();
            let decode_result = if ext == "avif" && avif_status.avifdec_available {
                run_avifdec(&input, &tmp_png).await
            } else {
                run_ffmpeg_to_png(&resolve_ffmpeg(&app), &input, &tmp_png, resize).await
            };
            if let Err(e) = decode_result {
                registry.0.lock().unwrap().remove(&preview_id);
                return Err(e);
            }

            check_cancel!();
            let quality_str = quality.to_string();
            let enc = avif::resolve_avifenc();
            let mut child = match Command::new(&enc)
                .args([&tmp_png, "-q", &quality_str, "-j", "8", "-o", &out])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = std::fs::remove_file(&tmp_png);
                    registry.0.lock().unwrap().remove(&preview_id);
                    return Err(format!("avifenc: {e}"));
                }
            };

            let enc_stderr = child.stderr.take().unwrap();
            let mut enc_lines = tokio::io::BufReader::new(enc_stderr).lines();
            loop {
                tokio::select! {
                    line = enc_lines.next_line() => match line {
                        Ok(Some(_)) => {}
                        _           => break,
                    },
                    _ = &mut kill_rx => {
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        registry.0.lock().unwrap().remove(&preview_id);
                        let _ = std::fs::remove_file(&tmp_png);
                        let _ = std::fs::remove_file(&out);
                        return Err("Cancelled".to_string());
                    }
                }
            }
            let _ = child.wait().await;
            let _ = std::fs::remove_file(&tmp_png);
            registry.0.lock().unwrap().remove(&preview_id);

            let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
            return Ok(PreviewResult { path: out, output_size: size });
        }
    }

    // ── AVIF input → avifdec → FFmpeg ────────────────────────────────────
    let (actual_input, tmp_avif_dec) = if ext == "avif" && output_fmt != "avif" {
        let avif_status = avif::check_status().await;
        if avif_status.avifdec_available {
            let tmp = format!("{}.dec.png", out);
            match run_avifdec(&input, &tmp).await {
                Ok(_) => (tmp.clone(), Some(tmp)),
                Err(_) => (input.clone(), None),
            }
        } else {
            (input.clone(), None)
        }
    } else {
        (input.clone(), None)
    };

    // ── FFmpeg path ───────────────────────────────────────────────────────
    let (mut args, _) = build_image_args(&opts, &actual_input);
    // Replace the last arg (output path) with the preview cache path
    if let Some(last) = args.last_mut() { *last = out.clone(); }

    check_cancel!();
    let mut ff_child = match Command::new(resolve_ffmpeg(&app))
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            if let Some(tmp) = tmp_avif_dec { let _ = std::fs::remove_file(tmp); }
            registry.0.lock().unwrap().remove(&preview_id);
            return Err(format!("ffmpeg: {e}"));
        }
    };

    let ff_stderr = ff_child.stderr.take().unwrap();
    let mut ff_lines = tokio::io::BufReader::new(ff_stderr).lines();
    let mut ff_stderr_buf: Vec<String> = Vec::new();
    loop {
        tokio::select! {
            line = ff_lines.next_line() => match line {
                Ok(Some(l)) => ff_stderr_buf.push(l),
                _           => break,
            },
            _ = &mut kill_rx => {
                let _ = ff_child.kill().await;
                let _ = ff_child.wait().await;
                if let Some(tmp) = tmp_avif_dec { let _ = std::fs::remove_file(tmp); }
                registry.0.lock().unwrap().remove(&preview_id);
                let _ = std::fs::remove_file(&out);
                return Err("Cancelled".to_string());
            }
        }
    }
    let ff_status = ff_child.wait().await.map_err(|e| format!("Wait error: {e}"))?;
    if let Some(tmp) = tmp_avif_dec { let _ = std::fs::remove_file(tmp); }
    registry.0.lock().unwrap().remove(&preview_id);

    if !ff_status.success() {
        let _ = std::fs::remove_file(&out);
        return Err(
            ff_stderr_buf.iter().rev()
                .find(|l| !l.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| "ffmpeg 失敗".to_string()),
        );
    }

    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    Ok(PreviewResult { path: out, output_size: size })
}

#[tauri::command]
async fn cleanup_preview_cache(app: AppHandle) -> Result<(), String> {
    let dir = preview_cache_dir(&app);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).ok();
    }
    Ok(())
}

/// Copy the output file to the system clipboard so it can be pasted into
/// other apps (e.g. Discord, Finder).  macOS uses AppleScript; Windows uses
/// PowerShell's Set-Clipboard; Linux is not supported.
#[tauri::command]
async fn copy_file_to_clipboard(path: String) -> Result<(), String> {
    if !std::path::Path::new(&path).exists() {
        return Err("ファイルが見つかりません".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        // Escape double-quotes and backslashes for the AppleScript string literal.
        let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("set the clipboard to POSIX file \"{}\"", escaped);
        let out = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await
            .map_err(|e| format!("osascript 起動失敗: {e}"))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(format!("クリップボードへのコピー失敗: {}", msg.trim()));
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        // PowerShell 5.1+ (bundled with Windows 10+)
        let escaped = path.replace('"', "\\\"");
        let script = format!(r#"Set-Clipboard -Path "{}""#, escaped);
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .await
            .map_err(|e| format!("PowerShell 起動失敗: {e}"))?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).to_string();
            return Err(format!("クリップボードへのコピー失敗: {}", msg.trim()));
        }
        return Ok(());
    }

    #[allow(unreachable_code)]
    Err("このプラットフォームはファイルのクリップボードコピーに対応していません".to_string())
}

// ── FFmpeg info command ───────────────────────────────────────────────────

#[derive(Serialize)]
struct FfmpegInfo {
    version: String,
    path: String,
    encoders: Vec<String>,
    decoders: Vec<String>,
    #[serde(rename = "buildFlags")]
    build_flags: Vec<String>,
}

fn parse_codec_names(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.starts_with("---") || l.is_empty() { return None; }
            // Format: " V..... name  description"
            let mut parts = l.split_whitespace();
            parts.next()?; // flags
            Some(parts.next()?.to_string())
        })
        .collect()
}

#[tauri::command]
async fn get_ffmpeg_info(app: AppHandle) -> Result<FfmpegInfo, String> {
    let ffmpeg = ffmpeg_dl::resolve_ffmpeg(&app);

    let ver_out = Command::new(&ffmpeg)
        .args(["-hide_banner", "-version"])
        .output()
        .await
        .map_err(|e| format!("ffmpeg version: {e}"))?;

    let version = String::from_utf8_lossy(&ver_out.stdout)
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(2))
        .unwrap_or("unknown")
        .to_string();

    let build_flags: Vec<String> = String::from_utf8_lossy(&ver_out.stdout)
        .lines()
        .flat_map(|l| l.split_whitespace())
        .filter(|s| s.starts_with("--enable-lib"))
        .map(|s| s.trim_start_matches("--enable-").to_string())
        .collect();

    let enc_out = Command::new(&ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .await
        .map_err(|e| format!("encoders: {e}"))?;

    let dec_out = Command::new(&ffmpeg)
        .args(["-hide_banner", "-decoders"])
        .output()
        .await
        .map_err(|e| format!("decoders: {e}"))?;

    Ok(FfmpegInfo {
        version,
        path: ffmpeg,
        encoders: parse_codec_names(&enc_out.stdout),
        decoders: parse_codec_names(&dec_out.stdout),
        build_flags,
    })
}

#[derive(Serialize)]
struct EncoderCaps {
    name: String,
    found: bool,
    #[serde(rename = "pixFmts")]
    pix_fmts: Vec<String>,
    #[serde(rename = "supportsAlpha")]
    supports_alpha: bool,
    #[serde(rename = "supportsHighDepth")]
    supports_high_depth: bool,
}

fn pix_fmt_high_depth(f: &str) -> bool {
    ["10le", "10be", "12le", "12be", "14le", "14be", "16le", "16be", "p010", "p016", "48", "64"]
        .iter()
        .any(|m| f.contains(m))
}

/// Query per-encoder capabilities (pixel formats → alpha / high bit depth).
/// Used by the codec status screen to surface library-dependent quirks.
#[tauri::command]
async fn get_encoder_caps(name: String, app: AppHandle) -> Result<EncoderCaps, String> {
    // Guard against argument injection — encoder names are [a-z0-9_-] only
    if name.is_empty()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("invalid encoder name".to_string());
    }

    let ffmpeg = ffmpeg_dl::resolve_ffmpeg(&app);
    let out = Command::new(&ffmpeg)
        .args(["-hide_banner", "-h", &format!("encoder={name}")])
        .output()
        .await
        .map_err(|e| format!("ffmpeg: {e}"))?;

    let text = String::from_utf8_lossy(&out.stdout);
    let found = text.contains(&format!("Encoder {name}"));

    let pix_fmts: Vec<String> = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("Supported pixel formats:"))
        .map(|s| s.split_whitespace().map(|x| x.to_string()).collect())
        .unwrap_or_default();

    let supports_alpha = pix_fmts.iter().any(|f| pix_fmt_has_alpha(f));
    let supports_high_depth = pix_fmts.iter().any(|f| pix_fmt_high_depth(f));

    Ok(EncoderCaps {
        name,
        found,
        pix_fmts,
        supports_alpha,
        supports_high_depth,
    })
}

// ── FFmpeg setup commands ─────────────────────────────────────────────────

#[tauri::command]
async fn check_ffmpeg(app: AppHandle) -> ffmpeg_dl::FfmpegStatus {
    ffmpeg_dl::check_status(&app).await
}

#[tauri::command]
async fn download_ffmpeg(
    app: AppHandle,
    on_progress: Channel<ffmpeg_dl::DownloadProgress>,
) -> Result<(), String> {
    ffmpeg_dl::do_download(&app, on_progress).await
}

// ── Tauri setup ───────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(ProcessRegistry(Arc::new(Mutex::new(HashMap::new()))))
        .invoke_handler(tauri::generate_handler![
            analyze_file,
            detect_hw_accel,
            process_file,
            cancel_process,
            get_ffmpeg_info,
            get_encoder_caps,
            check_ffmpeg,
            download_ffmpeg,
            check_avifenc,
            install_avifenc,
            save_from_preview,
            preview_image,
            cleanup_preview_cache,
            copy_file_to_clipboard,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
