//! `mixlens` command-line front end for the analysis engine.
//!
//!   mixlens analyze <file> [--master|--reference] [--platform <id> | --target <lufs>] [--genre <id>] [--key <key>] [--ai] [--full]
//!   mixlens compare <a> <b> [--reference] [--genre <id>] [--key <key>] [--ai]
//!   mixlens presets
//!   mixlens platforms
//!   mixlens section <file> <t0> <t1>
//!   mixlens codec <file> [--ffmpeg <path>]
//!   mixlens streaming <file> [--master|--reference] [--encode] [--ffmpeg <path>] [--ai]

use mixlens_core::{analyze_file, builtin_platforms, builtin_presets, codec, compare, reference_compare, streaming, tools, Audio, Kind, MusicalKey, Report, Targets};
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::exit;

const USAGE: &str = "usage:
  mixlens analyze <file> [--master|--reference] [--platform <id> | --target <lufs>] [--genre <id>] [--key <key, e.g. F#m>] [--ai] [--full]
  mixlens compare <a> <b> [--reference] [--genre <id>] [--key <key>] [--ai]   (--reference: b is a reference track; genre and key describe a)
  mixlens presets
  mixlens platforms
  mixlens section <file> <t0> <t1>
  mixlens codec <file> [--ffmpeg <path>]
  mixlens streaming <file> [--master|--reference] [--encode] [--ffmpeg <path>] [--ai]";

/// Options that take a value.
const VALUE_OPTS: [&str; 5] = ["--platform", "--target", "--genre", "--key", "--ffmpeg"];

fn usage() -> ! {
    eprintln!("{USAGE}");
    exit(2)
}

/// Bad invocation.
fn bad(msg: impl Display) -> ! {
    eprintln!("{msg}");
    exit(2)
}

/// Failure while running.
fn die(msg: impl Display) -> ! {
    eprintln!("error: {msg}");
    exit(1)
}

/// (positional count, allowed options) per command.
fn spec(cmd: &str) -> Option<(usize, &'static [&'static str])> {
    Some(match cmd {
        "analyze" => (1, &["--master", "--reference", "--platform", "--target", "--genre", "--key", "--ai", "--full"]),
        "compare" => (2, &["--reference", "--genre", "--key", "--ai"]),
        "presets" | "platforms" => (0, &[]),
        "section" => (3, &[]),
        "codec" => (1, &["--ffmpeg"]),
        "streaming" => (1, &["--master", "--reference", "--encode", "--ffmpeg", "--ai"]),
        _ => return None,
    })
}

struct Args {
    cmd: String,
    pos: Vec<String>,
    flags: HashSet<String>,
    opts: HashMap<String, String>,
}

impl Args {
    fn parse(mut raw: impl Iterator<Item = String>) -> Self {
        let cmd = raw.next().unwrap_or_else(|| usage());
        let (npos, allowed) = spec(&cmd).unwrap_or_else(|| usage());
        let (mut pos, mut flags, mut opts) = (vec![], HashSet::new(), HashMap::new());
        while let Some(a) = raw.next() {
            if VALUE_OPTS.contains(&a.as_str()) {
                let v = raw.next().unwrap_or_else(|| bad(format!("{a} needs a value")));
                opts.insert(a, v);
            } else if a.starts_with("--") {
                flags.insert(a);
            } else {
                pos.push(a);
            }
        }
        if let Some(o) = flags.iter().chain(opts.keys()).find(|o| !allowed.contains(&o.as_str())) {
            bad(format!("'{o}' isn't an option for `mixlens {cmd}`\n{USAGE}"));
        }
        if pos.len() != npos {
            usage();
        }
        Args { cmd, pos, flags, opts }
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.opts.get(name).map(String::as_str)
    }
}

fn targets(args: &Args) -> Targets {
    let mut t = Targets::default();
    match (args.flag("--master"), args.flag("--reference")) {
        (true, true) => bad("--master and --reference can't be combined"),
        (true, false) => t.kind = Kind::Master,
        (false, true) => t.kind = Kind::Reference,
        (false, false) => {}
    }
    if let Some(id) = args.opt("--platform") {
        if args.opt("--target").is_some() {
            bad("--platform and --target can't be combined; the platform sets the target");
        }
        let p = builtin_platforms().into_iter().find(|p| p.id == id).unwrap_or_else(|| bad(format!("unknown platform '{id}'; run `mixlens platforms` for the list")));
        t.target_lufs = p.target_lufs;
        t.ceiling_dbtp = p.ceiling_dbtp;
        t.loud_ceiling_dbtp = p.loud_ceiling_dbtp.unwrap_or(p.ceiling_dbtp);
        t.platform = Some(p);
    } else if let Some(v) = args.opt("--target") {
        t.target_lufs = v.parse().ok().filter(|x: &f64| x.is_finite()).unwrap_or_else(|| bad(format!("--target needs a loudness in LUFS, e.g. -14; got '{v}'")));
    }
    if let Some(id) = args.opt("--genre") {
        t.genre = Some(builtin_presets().into_iter().find(|p| p.id == id).unwrap_or_else(|| bad(format!("unknown genre '{id}'; run `mixlens presets` for the list"))));
    }
    if let Some(k) = args.opt("--key") {
        t.key = Some(MusicalKey::parse(k).unwrap_or_else(|| bad(format!("can't read key '{k}'; use e.g. C, F#m, Bb major, A minor"))));
    }
    t
}

fn analyze(path: &str, t: &Targets) -> (Audio, Report) {
    analyze_file(Path::new(path), t).unwrap_or_else(|e| die(format!("{path}: {e}")))
}

/// Analyze two files side by side.
fn analyze_pair(pa: &str, ta: &Targets, pb: &str, tb: &Targets) -> ((Audio, Report), (Audio, Report)) {
    std::thread::scope(|s| {
        let b = s.spawn(|| analyze(pb, tb));
        (analyze(pa, ta), b.join().unwrap())
    })
}

fn ffmpeg(args: &Args) -> PathBuf {
    codec::find_ffmpeg(args.opt("--ffmpeg")).unwrap_or_else(|| die("ffmpeg not found; install it or pass --ffmpeg <path>"))
}

fn main() {
    let args = Args::parse(std::env::args().skip(1));
    let targets = targets(&args);
    let ai = args.flag("--ai");

    let out = match args.cmd.as_str() {
        "analyze" => {
            let (_, r) = analyze(&args.pos[0], &targets);
            if ai {
                r.to_ai_json()
            } else {
                let mut v = serde_json::to_value(&r).unwrap();
                if !args.flag("--full") {
                    v.as_object_mut().unwrap().remove("kblocks");
                }
                v
            }
        }
        "compare" => {
            let (pa, pb) = (&args.pos[0], &args.pos[1]);
            let ta = Targets { kind: Kind::Mix, ..targets };
            if args.flag("--reference") {
                // The reference is another song: the version's genre and key don't describe it.
                let tb = Targets { kind: Kind::Reference, genre: None, key: None, ..ta.clone() };
                let ((_, ra), (_, rb)) = analyze_pair(pa, &ta, pb, &tb);
                let c = reference_compare(&ra, &rb);
                if ai {
                    c.to_ai_json()
                } else {
                    serde_json::to_value(&c).unwrap()
                }
            } else {
                let ((a, ra), (b, rb)) = analyze_pair(pa, &ta, pb, &ta);
                let c = compare(&a, &ra, &b, &rb).unwrap_or_else(|e| die(e));
                if ai {
                    c.to_ai_json()
                } else {
                    serde_json::to_value(&c).unwrap()
                }
            }
        }
        "section" => {
            let time = |s: &str| s.parse().ok().filter(|t: &f64| t.is_finite()).unwrap_or_else(|| bad(format!("'{s}' isn't a time in seconds")));
            let (t0, t1) = (time(&args.pos[1]), time(&args.pos[2]));
            if t0 < 0.0 || t1 <= t0 {
                bad("section needs 0 <= t0 < t1 (seconds)");
            }
            let (a, r) = analyze(&args.pos[0], &targets);
            let duration = r.file.duration_s;
            if t0 >= duration {
                bad(format!("t0 is past the end of the file ({duration:.1} s)"));
            }
            let t1 = t1.min(duration);
            // Loudness is gated over 400 ms blocks; anything shorter has no reading.
            if t1 - t0 < 0.4 {
                bad("section needs at least 0.4 s of audio");
            }
            serde_json::json!({
                "loudness": tools::section_loudness(&r, t0, t1),
                "bands": tools::band_energy(&a, t0, t1),
            })
        }
        "presets" => serde_json::to_value(builtin_presets()).unwrap(),
        "platforms" => serde_json::to_value(builtin_platforms()).unwrap(),
        "streaming" => {
            let path = &args.pos[0];
            let (_, r) = analyze(path, &targets);
            let encoded = args.flag("--encode").then(|| codec::codec_check(Path::new(path), &ffmpeg(&args)));
            let s = streaming::assess(&r, targets.kind, &builtin_platforms(), encoded.as_deref());
            if ai {
                s.to_ai_json()
            } else {
                serde_json::to_value(&s).unwrap()
            }
        }
        "codec" => {
            let path = &args.pos[0];
            if !Path::new(path).is_file() {
                die(format!("{path}: no such file"));
            }
            serde_json::to_value(codec::codec_check(Path::new(path), &ffmpeg(&args))).unwrap()
        }
        _ => unreachable!("Args::parse rejects unknown commands"),
    };

    // A closed pipe (e.g. `| head`) isn't an error.
    let mut stdout = std::io::stdout().lock();
    let written = serde_json::to_writer_pretty(&mut stdout, &out).map_err(std::io::Error::from).and_then(|()| writeln!(stdout));
    if let Err(e) = written {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            die(e);
        }
    }
}
