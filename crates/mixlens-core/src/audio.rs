//! Decoding audio files into planar f32 buffers.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    DecoderOptions, CODEC_TYPE_NULL, CODEC_TYPE_PCM_F32BE, CODEC_TYPE_PCM_F32LE, CODEC_TYPE_PCM_F64BE, CODEC_TYPE_PCM_F64LE,
    CODEC_TYPE_PCM_S16BE, CODEC_TYPE_PCM_S16LE, CODEC_TYPE_PCM_S24BE, CODEC_TYPE_PCM_S24LE, CODEC_TYPE_PCM_S32BE,
    CODEC_TYPE_PCM_S32LE,
};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::sample::SampleFormat;

/// Decoded audio. Always two channels (mono is duplicated, extra channels dropped).
#[derive(Clone)]
pub struct Audio {
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub info: FileInfo,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileInfo {
    pub path: String,
    pub file_name: String,
    pub sample_rate: u32,
    /// Channels in the source file.
    pub channels: u32,
    pub bits_per_sample: Option<u32>,
    /// e.g. "int16", "int24", "float32"
    pub sample_format: String,
    pub duration_s: f64,
    pub size_bytes: u64,
    /// BLAKE3 hash of the file bytes, used as a cache key.
    pub hash: String,
}

impl Audio {
    pub fn len(&self) -> usize {
        self.left.len()
    }
    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }
    pub fn mid(&self) -> Vec<f32> {
        self.left.iter().zip(&self.right).map(|(l, r)| 0.5 * (l + r)).collect()
    }
    pub fn seconds_to_index(&self, t: f64) -> usize {
        ((t.max(0.0) * self.sample_rate as f64) as usize).min(self.len())
    }
}

/// Hash a file without decoding it.
pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

pub fn decode_file(path: &Path) -> Result<Audio> {
    let bytes = std::fs::read(path)?;
    let size_bytes = bytes.len() as u64;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let file_name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| Error::Decode(format!("unsupported or unreadable file: {e}")))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Decode("no audio track".into()))?;
    let track_id = track.id;
    let params = track.codec_params.clone();
    let sample_rate = params.sample_rate.ok_or_else(|| Error::Decode("unknown sample rate".into()))?;
    let channels = params.channels.map(|c| c.count() as u32).unwrap_or(2);
    let bits = params.bits_per_sample.or(params.bits_per_coded_sample);
    let by_codec = match params.codec {
        c if c == CODEC_TYPE_PCM_F32LE || c == CODEC_TYPE_PCM_F32BE => Some("float32"),
        c if c == CODEC_TYPE_PCM_F64LE || c == CODEC_TYPE_PCM_F64BE => Some("float64"),
        c if c == CODEC_TYPE_PCM_S16LE || c == CODEC_TYPE_PCM_S16BE => Some("int16"),
        c if c == CODEC_TYPE_PCM_S24LE || c == CODEC_TYPE_PCM_S24BE => Some("int24"),
        c if c == CODEC_TYPE_PCM_S32LE || c == CODEC_TYPE_PCM_S32BE => Some("int32"),
        _ => None,
    };
    let sample_format = if let Some(f) = by_codec { f.to_string() } else { match params.sample_format {
        Some(SampleFormat::F32) => "float32".to_string(),
        Some(SampleFormat::F64) => "float64".to_string(),
        Some(SampleFormat::S16) | Some(SampleFormat::U16) => "int16".to_string(),
        Some(SampleFormat::S24) | Some(SampleFormat::U24) => "int24".to_string(),
        Some(SampleFormat::S32) | Some(SampleFormat::U32) => "int32".to_string(),
        Some(SampleFormat::S8) | Some(SampleFormat::U8) => "int8".to_string(),
        None => match bits {
            Some(b) => format!("int{b}"),
            None => "compressed".to_string(),
        },
    } };

    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| Error::Decode(format!("no decoder: {e}")))?;

    let cap = params.n_frames.unwrap_or(0) as usize;
    let mut left = Vec::with_capacity(cap);
    let mut right = Vec::with_capacity(cap);
    let mut buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(symphonia::core::errors::Error::ResetRequired) => break,
            Err(e) => return Err(Error::Decode(e.to_string())),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(Error::Decode(e.to_string())),
        };
        let spec = *decoded.spec();
        let nch = spec.channels.count().max(1);
        let sb = buf.get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        if sb.capacity() < decoded.capacity() * nch {
            *sb = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        sb.copy_interleaved_ref(decoded);
        let s = sb.samples();
        if nch == 1 {
            left.extend_from_slice(s);
            right.extend_from_slice(s);
        } else {
            for fr in s.chunks_exact(nch) {
                left.push(fr[0]);
                right.push(fr[1]);
            }
        }
    }
    if left.is_empty() {
        return Err(Error::Decode("file contains no audio".into()));
    }
    let duration_s = left.len() as f64 / sample_rate as f64;
    Ok(Audio {
        sample_rate,
        left,
        right,
        info: FileInfo {
            path: path.to_string_lossy().to_string(),
            file_name,
            sample_rate,
            channels,
            bits_per_sample: bits,
            sample_format,
            duration_s,
            size_bytes,
            hash,
        },
    })
}

/// Build an Audio from raw planar buffers (used by tests and resampling).
pub fn from_planar(sample_rate: u32, left: Vec<f32>, right: Vec<f32>, name: &str) -> Audio {
    let duration_s = left.len() as f64 / sample_rate as f64;
    Audio {
        sample_rate,
        info: FileInfo {
            path: name.to_string(),
            file_name: name.to_string(),
            sample_rate,
            channels: 2,
            bits_per_sample: None,
            sample_format: "float32".into(),
            duration_s,
            size_bytes: 0,
            hash: String::new(),
        },
        left,
        right,
    }
}
