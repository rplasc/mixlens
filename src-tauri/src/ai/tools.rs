//! Tools the AI can call. Each returns compact JSON computed by the DSP engine.

use super::ToolDef;
use crate::state::Inner;
use mixlens_core::report::round_floats;
use mixlens_core::{codec, compare, tools};
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
            description: "Align two versions sample-accurately and compare them: null-test residual, which time regions changed, per-section loudness deltas, per-octave energy changes in changed regions, third-octave balance deltas, and metric deltas. Use version_a for the older/reference version.",
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
            description: "Encode a version to Ogg Vorbis (q5, q9) and AAC (256k, 128k) with ffmpeg and measure decoded true peak and loudness. Use before approving a master for streaming. Takes ~10-20 s.",
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
            Ok(json!(codec::codec_check(Path::new(&v.path), &ff)))
        }
        other => Err(format!("unknown tool {other}")),
    }
}
