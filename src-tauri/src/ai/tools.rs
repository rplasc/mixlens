//! Tools the AI can call. Each returns compact JSON computed by the DSP engine.

use super::ToolDef;
use crate::state::Inner;
use mixlens_core::report::round_floats;
use mixlens_core::{codec, compare, reference_compare, tools, Kind};
use serde_json::{json, Value};
use std::path::Path;

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "get_report",
            description: "Full measurement report for one version: loudness, peaks, stereo, third-octave balance vs tilt, low-frequency peaks, sibilance, edges/silence, sections, dither, and rule-based findings.",
            schema: json!({"type": "object", "properties": {"version_id": {"type": "integer"}}, "required": ["version_id"]}),
        },
        ToolDef {
            name: "compare_versions",
            description: "Compare two versions. For two bounces of this song (mode null_test): sample-accurate alignment, null-test residual, which time regions changed, per-section loudness deltas, per-octave energy changes in changed regions, third-octave balance deltas, and metric deltas; use version_a for the older version. If either is a reference track (mode reference): level-match amount, octave balance vs the reference, level-matched third-octave difference, and loudness/dynamics/width deltas.",
            schema: json!({"type": "object", "properties": {"version_a": {"type": "integer"}, "version_b": {"type": "integer"}}, "required": ["version_a", "version_b"]}),
        },
        ToolDef {
            name: "section_analysis",
            description: "Loudness (BS.1770 gated) and spectral balance for a time range of one version, e.g. a chorus or the outro.",
            schema: json!({"type": "object", "properties": {
                "version_id": {"type": "integer"},
                "start_s": {"type": "number"},
                "end_s": {"type": "number"}
            }, "required": ["version_id", "start_s", "end_s"]}),
        },
        ToolDef {
            name: "codec_check",
            description: "Encode a version to Ogg Vorbis (q5, q9), AAC (256k, 128k), Opus 128k and MP3 320k with ffmpeg and measure decoded true peak and loudness. Use before approving a master for streaming; streaming_check then uses these peaks. Takes ~10-20 s.",
            schema: json!({"type": "object", "properties": {"version_id": {"type": "integer"}}, "required": ["version_id"]}),
        },
        ToolDef {
            name: "streaming_check",
            description: "Per-platform streaming breakdown (Spotify incl. Loud/Quiet modes, Apple Music, YouTube, Amazon Music, Tidal, Deezer): how much each turns the track up or down, the resulting playback loudness and shortfall, true-peak ceilings, peaks after gain and encoding, plus clipping and delivery-format checks. Run codec_check first for post-encode peaks.",
            schema: json!({"type": "object", "properties": {"version_id": {"type": "integer"}}, "required": ["version_id"]}),
        },
    ]
}

fn id(args: &Value, k: &str) -> Result<i64, String> {
    args[k].as_i64().or_else(|| args[k].as_f64().map(|f| f as i64)).ok_or_else(|| format!("missing integer '{k}'"))
}

fn num(args: &Value, k: &str) -> Result<f64, String> {
    args[k].as_f64().ok_or_else(|| format!("missing number '{k}'"))
}

/// Runs a tool synchronously (call from a blocking thread).
pub fn execute(inner: &Inner, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "get_report" => {
            let v = inner.version(id(args, "version_id")?)?;
            let mut out = v.report.to_ai_json();
            out["version"] = json!({"id": v.id, "label": v.label, "kind": v.kind});
            Ok(out)
        }
        "compare_versions" => {
            let (ia, ib) = (id(args, "version_a")?, id(args, "version_b")?);
            let (va, vb) = (inner.version(ia)?, inner.version(ib)?);
            if va.kind == Kind::Reference || vb.kind == Kind::Reference {
                // A reference is a different song: compare balance and dynamics, never null-test.
                let (version, reference) = if va.kind == Kind::Reference && vb.kind != Kind::Reference { (vb, va) } else { (va, vb) };
                let mut out = reference_compare(&version.report, &reference.report).to_ai_json();
                out["version"] = json!({"id": version.id, "label": version.label});
                out["reference"] = json!({"id": reference.id, "label": reference.label});
                return Ok(out);
            }
            let (aa, ab) = (inner.audio(ia)?, inner.audio(ib)?);
            let c = compare(&aa, &va.report, &ab, &vb.report).map_err(|e| e.to_string())?;
            let mut out = c.to_ai_json();
            out["a"] = json!({"id": va.id, "label": va.label});
            out["b"] = json!({"id": vb.id, "label": vb.label});
            Ok(out)
        }
        "section_analysis" => {
            let vid = id(args, "version_id")?;
            let (t0, t1) = (num(args, "start_s")?, num(args, "end_s")?);
            if t1 <= t0 {
                return Err("end_s must be after start_s".into());
            }
            let v = inner.version(vid)?;
            let a = inner.audio(vid)?;
            let mut out = json!({
                "loudness": tools::section_loudness(&v.report, t0, t1),
                "bands": tools::band_energy(&a, t0, t1),
                "octave_bands": compare::octave_band_labels(),
            });
            round_floats(&mut out);
            Ok(out)
        }
        "codec_check" => {
            let v = inner.version(id(args, "version_id")?)?;
            let s = inner.settings();
            let ff = codec::find_ffmpeg(Some(&s.ffmpeg_path).filter(|p| !p.is_empty()).map(|p| p.as_str()))
                .ok_or("ffmpeg not found. Install it (winget install Gyan.FFmpeg) or set its path in Settings.")?;
            if !Path::new(&v.path).exists() {
                return Err(format!("{} not found on disk", v.path));
            }
            let results = codec::codec_check(Path::new(&v.path), &ff);
            inner.codec_results.lock().unwrap().insert(v.id, results.clone());
            Ok(json!(results))
        }
        "streaming_check" => {
            let mut out = inner.streaming(id(args, "version_id")?)?.to_ai_json();
            out["main_platform"] = json!(inner.settings().main_platform);
            Ok(out)
        }
        other => Err(format!("unknown tool {other}")),
    }
}
