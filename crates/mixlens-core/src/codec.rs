//! Lossy-encode preview: encode with ffmpeg and measure the decoded true peak.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodecResult {
    pub codec: String,
    pub label: String,
    pub true_peak_dbtp: Option<f64>,
    pub integrated_lufs: Option<f64>,
    pub error: Option<String>,
}

/// (id, label, ffmpeg encoder args, container). Platform cards map their streams to these ids.
pub const PRESETS: [(&str, &str, &[&str], &str); 6] = [
    ("ogg_q5", "Ogg Vorbis q5 (~160 kbps, Spotify High)", &["-c:a", "libvorbis", "-q:a", "5"], "ogg"),
    ("ogg_q9", "Ogg Vorbis q9 (~320 kbps, Spotify Very High)", &["-c:a", "libvorbis", "-q:a", "9"], "ogg"),
    ("aac_256", "AAC 256 kbps (Apple Music, Amazon, Tidal)", &["-c:a", "aac", "-b:a", "256k"], "m4a"),
    ("aac_128", "AAC 128 kbps (low-bitrate streams)", &["-c:a", "aac", "-b:a", "128k"], "m4a"),
    ("opus_128", "Opus 128 kbps (YouTube)", &["-c:a", "libopus", "-b:a", "128k"], "opus"),
    ("mp3_320", "MP3 320 kbps (Deezer)", &["-c:a", "libmp3lame", "-b:a", "320k"], "mp3"),
];

fn command(bin: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c.stdin(Stdio::null());
    c
}

/// Use the configured path, else look for ffmpeg on PATH.
pub fn find_ffmpeg(custom: Option<&str>) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = match custom.filter(|s| !s.trim().is_empty()) {
        Some(p) => vec![PathBuf::from(p.trim())],
        None => vec![PathBuf::from(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })],
    };
    candidates.into_iter().find(|p| {
        command(p).arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
    })
}

/// Parse the summary block of ffmpeg's ebur128 filter.
pub fn parse_ebur128_summary(stderr: &str) -> (Option<f64>, Option<f64>) {
    let summary = match stderr.rfind("Summary:") {
        Some(i) => &stderr[i..],
        None => return (None, None),
    };
    let num = |line: &str| line.split_whitespace().nth(1).and_then(|t| t.parse::<f64>().ok());
    let (mut i, mut p) = (None, None);
    for line in summary.lines() {
        let t = line.trim();
        if t.starts_with("I:") && i.is_none() {
            i = num(t);
        } else if t.starts_with("Peak:") {
            p = num(t);
        }
    }
    (i, p)
}

fn measure(ffmpeg: &Path, file: &Path) -> Result<(Option<f64>, Option<f64>), String> {
    let out = command(ffmpeg)
        .args(["-hide_banner", "-nostats", "-i"])
        .arg(file)
        .args(["-af", "ebur128=peak=true", "-f", "null", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(parse_ebur128_summary(&String::from_utf8_lossy(&out.stderr)))
}

pub fn codec_check(input: &Path, ffmpeg: &Path) -> Vec<CodecResult> {
    let dir = std::env::temp_dir().join(format!("mixlens-codec-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let results: Vec<CodecResult> = std::thread::scope(|s| {
        let handles: Vec<_> = PRESETS
            .iter()
            .map(|(code, label, args, ext)| {
                let dir = dir.clone();
                s.spawn(move || {
                    let out = dir.join(format!("{code}.{ext}"));
                    let enc = command(ffmpeg)
                        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
                        .arg(input)
                        .args(*args)
                        .arg(&out)
                        .output();
                    let mut r = CodecResult { codec: code.to_string(), label: label.to_string(), true_peak_dbtp: None, integrated_lufs: None, error: None };
                    match enc {
                        Ok(o) if o.status.success() => match measure(ffmpeg, &out) {
                            Ok((i, p)) => {
                                r.integrated_lufs = i;
                                r.true_peak_dbtp = p;
                            }
                            Err(e) => r.error = Some(e),
                        },
                        Ok(o) => r.error = Some(String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("encode failed").to_string()),
                        Err(e) => r.error = Some(e.to_string()),
                    }
                    let _ = std::fs::remove_file(&out);
                    r
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("codec thread")).collect()
    });
    let _ = std::fs::remove_dir(&dir);
    results
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_summary() {
        let s = "[Parsed_ebur128_0 @ 0x1] t: 1 I: -70 LUFS\n[Parsed_ebur128_0 @ 0x1] Summary:\n\n  Integrated loudness:\n    I:         -14.2 LUFS\n    Threshold: -24.4 LUFS\n\n  True peak:\n    Peak:       -0.8 dBFS\n";
        let (i, p) = super::parse_ebur128_summary(s);
        assert_eq!(i, Some(-14.2));
        assert_eq!(p, Some(-0.8));
    }
}
