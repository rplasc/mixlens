//! `mixlens` command-line front end for the analysis engine.
//!
//!   mixlens analyze <file> [--master|--reference] [--platform <id> | --target -14] [--genre <id>] [--key <key>] [--ai]
//!   mixlens compare <a> <b> [--reference] [--ai]
//!   mixlens presets
//!   mixlens section <file> <t0> <t1>
//!   mixlens codec <file> [--ffmpeg <path>]
//!   mixlens streaming <file> [--master|--reference] [--encode] [--ffmpeg <path>] [--ai]
//!   mixlens platforms

use mixlens_core::{analyze_file, builtin_platforms, builtin_presets, codec, compare, reference_compare, streaming, tools, Kind, MusicalKey, Targets};
use std::path::Path;
use std::process::exit;

fn usage() -> ! {
    eprintln!(
        "usage:\n  mixlens analyze <file> [--master|--reference] [--platform <id> | --target <lufs>] [--genre <id>] [--key <key, e.g. F#m>] [--ai]\n  mixlens compare <a> <b> [--reference] [--ai]   (--reference: b is a reference track)\n  mixlens presets\n  mixlens section <file> <t0> <t1>\n  mixlens codec <file> [--ffmpeg <path>]\n  mixlens streaming <file> [--master|--reference] [--encode] [--ffmpeg <path>] [--ai]\n  mixlens platforms"
    );
    exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }
    let flag = |name: &str| args.iter().any(|a| a == name);
    let opt = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let mut targets = Targets::default();
    if flag("--master") {
        targets.kind = Kind::Master;
    }
    if let Some(t) = opt("--target").and_then(|t| t.parse().ok()) {
        targets.target_lufs = t;
    }
    if let Some(id) = opt("--platform") {
        let p = builtin_platforms().into_iter().find(|p| p.id == id).unwrap_or_else(|| {
            eprintln!("unknown platform '{id}'; run `mixlens platforms` for the list");
            exit(2)
        });
        targets.target_lufs = p.target_lufs;
        targets.ceiling_dbtp = p.ceiling_dbtp;
        targets.loud_ceiling_dbtp = p.loud_ceiling_dbtp.unwrap_or(p.ceiling_dbtp);
        targets.platform = Some(p);
    }
    if flag("--reference") {
        targets.kind = Kind::Reference;
    }
    if let Some(id) = opt("--genre") {
        targets.genre = Some(builtin_presets().into_iter().find(|p| p.id == id).unwrap_or_else(|| {
            eprintln!("unknown genre '{id}'; run `mixlens presets` for the list");
            exit(2)
        }));
    }
    if let Some(k) = opt("--key") {
        targets.key = Some(MusicalKey::parse(&k).unwrap_or_else(|| {
            eprintln!("can't read key '{k}'; use e.g. C, F#m, Bb major, A minor");
            exit(2)
        }));
    }
    let ai = flag("--ai");
    let die = |e: mixlens_core::Error| -> ! {
        eprintln!("error: {e}");
        exit(1)
    };

    let out = match args[0].as_str() {
        "analyze" => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let (_, r) = analyze_file(Path::new(path), &targets).unwrap_or_else(|e| die(e));
            if ai {
                r.to_ai_json()
            } else {
                let mut v = serde_json::to_value(&r).unwrap();
                if !flag("--full") {
                    v.as_object_mut().unwrap().remove("kblocks");
                }
                v
            }
        }
        "compare" => {
            let (pa, pb) = (args.get(1).unwrap_or_else(|| usage()), args.get(2).unwrap_or_else(|| usage()));
            let ((a, ra), (b, rb)) = rayon_join(pa, pb, &targets);
            if flag("--reference") {
                let c = reference_compare(&ra, &rb);
                println!("{}", serde_json::to_string_pretty(&if ai { c.to_ai_json() } else { serde_json::to_value(&c).unwrap() }).unwrap());
                return;
            }
            let c = compare(&a, &ra, &b, &rb).unwrap_or_else(|e| die(e));
            if ai {
                c.to_ai_json()
            } else {
                serde_json::to_value(&c).unwrap()
            }
        }
        "section" => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let t0: f64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or_else(|| usage());
            let t1: f64 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or_else(|| usage());
            let (a, r) = analyze_file(Path::new(path), &targets).unwrap_or_else(|e| die(e));
            serde_json::json!({
                "loudness": tools::section_loudness(&r, t0, t1),
                "bands": tools::band_energy(&a, t0, t1),
            })
        }
        "presets" => serde_json::to_value(builtin_presets()).unwrap(),
        "platforms" => serde_json::to_value(builtin_platforms()).unwrap(),
        "streaming" => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let (_, r) = analyze_file(Path::new(path), &targets).unwrap_or_else(|e| die(e));
            let encoded = flag("--encode").then(|| {
                let ff = codec::find_ffmpeg(opt("--ffmpeg").as_deref()).unwrap_or_else(|| {
                    eprintln!("ffmpeg not found; install it or pass --ffmpeg <path>");
                    exit(1)
                });
                codec::codec_check(Path::new(path), &ff)
            });
            let s = streaming::assess(&r, targets.kind, &builtin_platforms(), encoded.as_deref());
            if ai {
                s.to_ai_json()
            } else {
                serde_json::to_value(&s).unwrap()
            }
        }
        "codec" => {
            let path = args.get(1).unwrap_or_else(|| usage());
            let ff = codec::find_ffmpeg(opt("--ffmpeg").as_deref()).unwrap_or_else(|| {
                eprintln!("ffmpeg not found; install it or pass --ffmpeg <path>");
                exit(1)
            });
            serde_json::to_value(codec::codec_check(Path::new(path), &ff)).unwrap()
        }
        _ => usage(),
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

fn rayon_join(pa: &str, pb: &str, t: &Targets) -> ((mixlens_core::Audio, mixlens_core::Report), (mixlens_core::Audio, mixlens_core::Report)) {
    let (a, b) = std::thread::scope(|s| {
        let ha = s.spawn(|| analyze_file(Path::new(pa), t));
        let hb = s.spawn(|| analyze_file(Path::new(pb), t));
        (ha.join().unwrap(), hb.join().unwrap())
    });
    match (a, b) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            exit(1)
        }
    }
}
