//! End-to-end test of the AI tool loop against a mock HTTP server for each provider.
//! Uses real analysis on generated WAV files, so tool results are genuine.

use crate::ai::{converse, Msg};
use crate::commands::ExportFormat;
use crate::db;
use crate::settings::{Provider, Settings};
use crate::state::{Inner, Shared};
use mixlens_core::{analyze_file, Kind, Targets};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn write_wav(path: &Path, freq: f32, amp: f32, secs: f32) {
    let sr = 48000u32;
    let n = (sr as f32 * secs) as usize;
    let mut data = Vec::with_capacity(n * 4);
    for i in 0..n {
        let t = i as f32 / sr as f32;
        let env = (t / secs * std::f32::consts::PI).sin();
        let v = (amp * env * (2.0 * std::f32::consts::PI * freq * t).sin() * 32767.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
        data.extend_from_slice(&v.to_le_bytes());
    }
    let mut f = std::fs::File::create(path).unwrap();
    let len = data.len() as u32;
    f.write_all(b"RIFF").unwrap();
    f.write_all(&(36 + len).to_le_bytes()).unwrap();
    f.write_all(b"WAVEfmt ").unwrap();
    f.write_all(&16u32.to_le_bytes()).unwrap();
    f.write_all(&1u16.to_le_bytes()).unwrap();
    f.write_all(&2u16.to_le_bytes()).unwrap();
    f.write_all(&sr.to_le_bytes()).unwrap();
    f.write_all(&(sr * 4).to_le_bytes()).unwrap();
    f.write_all(&4u16.to_le_bytes()).unwrap();
    f.write_all(&16u16.to_le_bytes()).unwrap();
    f.write_all(b"data").unwrap();
    f.write_all(&len.to_le_bytes()).unwrap();
    f.write_all(&data).unwrap();
}

/// Serves the canned responses in order, one per connection, and records request bodies.
fn mock_server(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(vec![]));
    let seen2 = seen.clone();
    std::thread::spawn(move || {
        for body in responses {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
            }
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).unwrap();
            seen2.lock().unwrap().push(String::from_utf8_lossy(&buf).to_string());
            let mut s = stream;
            write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    (addr, seen)
}

fn setup() -> (Shared, tempdir::TempDir) {
    let dir = tempdir::TempDir::new();
    let a = dir.0.join("song_mix_v1.wav");
    let b = dir.0.join("song_mix_v2.wav");
    write_wav(&a, 220.0, 0.3, 6.0);
    write_wav(&b, 220.0, 0.45, 6.0);
    let conn = db::open(&dir.0.join("t.db")).unwrap();
    let inner = Arc::new(Inner::new(conn, reqwest::Client::new()));
    {
        let c = inner.db.lock().unwrap();
        let p = db::create_project(&c, "Test").unwrap();
        for (path, label) in [(&a, "v1"), (&b, "v2")] {
            let (_, r) = analyze_file(path, &Targets::default()).unwrap();
            db::insert_version(&c, p.id, label, path.to_str().unwrap(), Kind::Mix, &r).unwrap();
        }
    }
    (inner, dir)
}

mod tempdir {
    pub struct TempDir(pub std::path::PathBuf);
    impl TempDir {
        pub fn new() -> Self {
            let p = std::env::temp_dir().join(format!("mixlens-test-{}-{}", std::process::id(), rand()));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn rand() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }
}

fn run(inner: Shared, provider: Provider, st: &Settings) -> String {
    let msgs = vec![Msg::User("What changed between v1 and v2?".into())];
    tauri::async_runtime::block_on(converse(inner, st, provider, "test-model", Some("test-key"), msgs, |_| {})).unwrap()
}

#[test]
fn agent_loop_all_providers() {
    let (inner, _dir) = setup();

    // Anthropic: tool_use then text.
    let (url, seen) = mock_server(vec![
        r#"{"content":[{"type":"text","text":"Comparing."},{"type":"tool_use","id":"toolu_1","name":"compare_versions","input":{"version_a":1,"version_b":2}}],"stop_reason":"tool_use"}"#.into(),
        r#"{"content":[{"type":"text","text":"done-anthropic"}],"stop_reason":"end_turn"}"#.into(),
    ]);
    std::env::set_var("MIXLENS_ANTHROPIC_BASE_URL", &url);
    let out = run(inner.clone(), Provider::Anthropic, &Settings::default());
    assert_eq!(out, "done-anthropic");
    let reqs = seen.lock().unwrap().clone();
    assert!(reqs[0].contains("cache_control") && reqs[0].contains("compare_versions"), "first request should carry cached system + tools");
    assert!(reqs[1].contains("tool_result") && reqs[1].contains("toolu_1"), "tool result must reference the call id");
    assert!(reqs[1].contains("residual_rel_db") && reqs[1].contains("section_deltas"), "real comparison data goes back to the model");

    // OpenAI: function call with string arguments, then text.
    let (url, seen) = mock_server(vec![
        r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_9","type":"function","function":{"name":"get_report","arguments":"{\"version_id\":2}"}}]}}]}"#.into(),
        r#"{"choices":[{"message":{"role":"assistant","content":"done-openai"}}]}"#.into(),
    ]);
    std::env::set_var("MIXLENS_OPENAI_BASE_URL", &url);
    let out = run(inner.clone(), Provider::Openai, &Settings::default());
    assert_eq!(out, "done-openai");
    let reqs = seen.lock().unwrap().clone();
    assert!(reqs[0].contains(r#""role":"system""#) && reqs[0].contains(r#""type":"function""#));
    assert!(reqs[1].contains(r#""tool_call_id":"call_9""#) && reqs[1].contains("integrated_lufs"));
    assert!(!reqs[1].contains("kblocks"), "bulky arrays are stripped before sending");

    // Ollama: object arguments, no ids, then text.
    let (url, seen) = mock_server(vec![
        r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"section_analysis","arguments":{"version_id":2,"start_s":1.0,"end_s":4.0}}}]}}"#.into(),
        r#"{"message":{"role":"assistant","content":"done-ollama"}}"#.into(),
    ]);
    let st = Settings { ollama_url: url, ..Settings::default() };
    let out = run(inner.clone(), Provider::Ollama, &st);
    assert_eq!(out, "done-ollama");
    let reqs = seen.lock().unwrap().clone();
    assert!(reqs[0].contains(r#""stream":false"#));
    assert!(reqs[1].contains(r#""tool_name":"section_analysis""#) && reqs[1].contains("vs_integrated_lu"));

    // KoboldCpp-style: tool call written as JSON text in content, no tool_calls field.
    let (url, seen) = mock_server(vec![
        r#"{"message":{"role":"assistant","content":"[\n{\n\"id\": \"call_001\",\n\"type\": \"function\",\n\"function\": {\n\"name\": \"get_report\",\n\"arguments\": {\n\"version_id\": 1\n}\n}\n}\n]"}}"#.into(),
        r#"{"message":{"role":"assistant","content":"done-text-call"}}"#.into(),
    ]);
    let st = Settings { ollama_url: url, ..Settings::default() };
    assert_eq!(run(inner.clone(), Provider::Ollama, &st), "done-text-call");
    assert!(seen.lock().unwrap()[1].contains(r#""tool_name":"get_report""#));

    // Tool errors are reported back to the model instead of failing the turn.
    let (url, seen) = mock_server(vec![
        r#"{"message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"get_report","arguments":{"version_id":999}}}]}}"#.into(),
        r#"{"message":{"role":"assistant","content":"recovered"}}"#.into(),
    ]);
    let st = Settings { ollama_url: url, ..Settings::default() };
    assert_eq!(run(inner, Provider::Ollama, &st), "recovered");
    assert!(seen.lock().unwrap()[1].contains("version 999 not found"));
}

#[test]
fn migrates_old_database() {
    let dir = tempdir::TempDir::new();
    let path = dir.0.join("old.db");
    {
        // Schema as shipped before song context existed.
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE projects (id INTEGER PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT (datetime('now')));
             INSERT INTO projects (name) VALUES ('Old song');",
        )
        .unwrap();
    }
    for _ in 0..2 {
        // Opening twice checks the migration is idempotent.
        let c = db::open(&path).unwrap();
        let p = &db::list_projects(&c).unwrap()[0];
        assert_eq!(p.name, "Old song");
        assert!(p.genre.is_none() && p.key.is_none());
    }
    let c = db::open(&path).unwrap();
    let key = mixlens_core::MusicalKey::parse("F#m");
    db::set_project_context(&c, 1, Some("hiphop"), key.as_ref()).unwrap();
    let p = db::get_project(&c, 1).unwrap().unwrap();
    assert_eq!(p.genre.as_deref(), Some("hiphop"));
    assert_eq!(p.key, key);
    let t = crate::state::targets_in(&c, 1, Kind::Mix);
    assert_eq!(t.genre.map(|g| g.id), Some("hiphop".into()));
    assert_eq!(t.key, key);
}

#[test]
fn reference_versions() {
    use crate::commands::guess_kind;
    assert_eq!(guess_kind("Artist - Song (ref)"), Kind::Reference);
    assert_eq!(guess_kind("ref_track"), Kind::Reference);
    assert_eq!(guess_kind("song_master_v3"), Kind::Master);
    assert_eq!(guess_kind("refrain_mix"), Kind::Mix);

    let (inner, dir) = setup();
    let r_path = dir.0.join("other_song_reference.wav");
    write_wav(&r_path, 110.0, 0.6, 6.0);
    {
        let c = inner.db.lock().unwrap();
        let t = Targets { kind: Kind::Reference, ..Targets::default() };
        let (_, r) = analyze_file(&r_path, &t).unwrap();
        assert_eq!(r.findings.len(), 1, "references skip the mix/master rules");
        let v = db::insert_version(&c, 1, "ref", r_path.to_str().unwrap(), Kind::Reference, &r).unwrap();
        assert_eq!(db::get_version(&c, v.id).unwrap().unwrap().kind, Kind::Reference);
    }
    // Either argument order: the reference is detected by kind, no null test runs.
    for args in [serde_json::json!({"version_a": 3, "version_b": 1}), serde_json::json!({"version_a": 1, "version_b": 3})] {
        let out = crate::ai::tools::execute(&inner, "compare_versions", &args).unwrap();
        assert_eq!(out["mode"], "reference");
        assert_eq!(out["reference"]["id"], 3);
        assert!(out.get("residual_rel_db").is_none());
    }
    let out = crate::ai::tools::execute(&inner, "compare_versions", &serde_json::json!({"version_a": 1, "version_b": 2})).unwrap();
    assert_eq!(out["mode"], "null_test");
}

#[test]
fn reference_has_own_context() {
    use crate::commands::{refresh_project, set_version_context, set_version_kind};
    let (inner, _dir) = setup();
    let c = inner.db.lock().unwrap();
    let key = |s: &str| mixlens_core::MusicalKey::parse(s);
    db::set_project_context(&c, 1, Some("hiphop"), key("F#m").as_ref()).unwrap();
    let ctx = |id: i64| {
        let r = db::get_version(&c, id).unwrap().unwrap().report;
        (r.genre_match.map(|g| g.preset_id), r.key_analysis.map(|k| k.key_name))
    };

    // Turning a version into a reference starts it with the song's context.
    let v = set_version_kind(&c, 2, "ref", Kind::Reference).unwrap();
    assert_eq!((v.genre.as_deref(), v.key), (Some("hiphop"), key("F#m")));
    assert_eq!(ctx(2), (Some("hiphop".into()), Some("F# minor".into())));

    // Its own context then applies to it alone.
    let v = set_version_context(&c, 2, Some("pop".into()), key("G")).unwrap();
    assert_eq!(v.genre.as_deref(), Some("pop"));
    assert_eq!(ctx(2), (Some("pop".into()), Some("G major".into())));

    // Changing the song's context leaves the reference alone.
    db::set_project_context(&c, 1, Some("edm"), key("Am").as_ref()).unwrap();
    let p = db::get_project(&c, 1).unwrap().unwrap();
    refresh_project(&c, &p, &crate::state::load_settings(&c), &crate::state::load_presets(&c)).unwrap();
    assert_eq!(ctx(1), (Some("edm".into()), Some("A minor".into())));
    assert_eq!(ctx(2), (Some("pop".into()), Some("G major".into())));

    // Bounces of the song can't have their own.
    assert!(set_version_context(&c, 1, Some("pop".into()), None).is_err());
    assert!(set_version_context(&c, 2, Some("nope".into()), None).is_err());
    // Switching back to a mix uses the song's context again.
    set_version_kind(&c, 2, "v2", Kind::Mix).unwrap();
    assert_eq!(ctx(2), (Some("edm".into()), Some("A minor".into())));

    // The comparison carries both sides' context.
    set_version_kind(&c, 2, "ref", Kind::Reference).unwrap();
    drop(c);
    let out = crate::ai::tools::execute(&inner, "compare_versions", &serde_json::json!({"version_a": 1, "version_b": 2})).unwrap();
    assert_eq!((out["version_key"].as_str(), out["reference_key"].as_str()), (Some("A minor"), Some("G major")));
}

#[test]
fn deleting_a_song_removes_its_versions_and_chat() {
    let (inner, _dir) = setup();
    let c = inner.db.lock().unwrap();
    let other = db::create_project(&c, "Other").unwrap();
    db::add_message(&c, 1, "user", "hi").unwrap();
    db::delete_project(&c, 1).unwrap();
    assert!(db::get_project(&c, 1).unwrap().is_none());
    assert!(db::list_versions(&c, 1).unwrap().is_empty() && db::get_version(&c, 1).unwrap().is_none());
    assert!(db::messages(&c, 1).unwrap().is_empty());
    assert_eq!(db::list_projects(&c).unwrap().iter().map(|p| p.id).collect::<Vec<_>>(), vec![other.id]);
}

#[test]
fn text_tool_call_parsing() {
    use crate::ai::{providers::text_tool_calls, tools};
    let defs = tools::definitions();
    let one = |s: &str| text_tool_calls(s, &defs).map(|c| (c[0].name.clone(), c[0].args.clone()));
    let want = Some(("get_report".to_string(), serde_json::json!({"version_id": 1})));
    assert_eq!(one(r#"{"name":"get_report","arguments":{"version_id":1}}"#), want);
    assert_eq!(one(r#"{"name":"get_report","parameters":{"version_id":1}}"#), want);
    assert_eq!(one("```json\n{\"function\":{\"name\":\"get_report\",\"arguments\":\"{\\\"version_id\\\":1}\"}}\n```"), want);
    assert_eq!(one("<tool_call>\n{\"name\":\"get_report\",\"arguments\":{\"version_id\":1}}\n</tool_call>"), want);
    // Ordinary answers and unknown tools stay as text.
    assert_eq!(one("Your master is fine. Next: check the codec."), None);
    assert_eq!(one(r#"{"name":"delete_everything","arguments":{}}"#), None);
    assert_eq!(one(r#"{"integrated_lufs": -14.0}"#), None);
}

#[test]
fn main_platform_settings() {
    use crate::state::{build_targets, effective_target, parse_settings};
    // Settings saved before "main platform" existed keep their behaviour.
    assert_eq!(parse_settings(r#"{"target_lufs": -14.0}"#).main_platform, "spotify");
    let custom = parse_settings(r#"{"target_lufs": -16.0}"#);
    assert_eq!((custom.main_platform.as_str(), effective_target(&custom)), ("custom", -16.0));
    assert_eq!(parse_settings(r#"{"target_lufs": -16.0, "main_platform": "youtube"}"#).main_platform, "youtube");

    let st = Settings { main_platform: "amazon".into(), ..Settings::default() };
    let t = build_targets(&st, &[], None, None, Kind::Master);
    assert_eq!((t.target_lufs, t.ceiling_dbtp, t.loud_ceiling_dbtp), (-14.0, -2.0, -2.0));
    assert_eq!(t.platform.map(|p| p.id), Some("amazon".into()));
    let t = build_targets(&custom, &[], None, None, Kind::Master);
    assert!(t.platform.is_none() && t.target_lufs == -16.0);
}

#[test]
fn streaming_check_tool() {
    use mixlens_core::codec::CodecResult;
    let (inner, _dir) = setup();
    let out = crate::ai::tools::execute(&inner, "streaming_check", &serde_json::json!({"version_id": 1})).unwrap();
    assert_eq!(out["platforms"].as_array().unwrap().len(), 6);
    assert_eq!(out["encoded"], false);
    assert_eq!(out["main_platform"], "spotify");

    // Encode results from codec_check are picked up for the matching platforms.
    let enc = |codec: &str, tp: f64| CodecResult { codec: codec.into(), label: codec.into(), true_peak_dbtp: Some(tp), integrated_lufs: None, error: None };
    inner.codec_results.lock().unwrap().insert(1, vec![enc("ogg_q9", 0.7), enc("aac_256", -3.0)]);
    let s = inner.streaming(1).unwrap();
    assert!(s.encoded);
    let peak = |id: &str| s.platforms.iter().find(|p| p.id == id).unwrap().encoded_peak_dbtp;
    assert_eq!((peak("spotify"), peak("apple"), peak("deezer")), (Some(0.7), Some(-3.0), None));
    assert!(s.general.iter().any(|f| f.code == "encode_clipping"));
    inner.forget(&[1]);
    assert!(!inner.streaming(1).unwrap().encoded);
}

#[test]
fn exports_report_as_markdown_and_pdf() {
    let (inner, dir) = setup();
    let id = db::list_versions(&inner.db.lock().unwrap(), 1).unwrap()[1].id;
    let export = |name: &str, format| crate::commands::export_report_to(&inner, id, dir.0.join(name).to_str().unwrap(), format);

    // The format decides the contents; a missing or different extension gets the right one added.
    let md = export("report", ExportFormat::Markdown).unwrap();
    assert!(md.ends_with("report.md"), "{md}");
    let md = std::fs::read_to_string(md).unwrap();
    assert!(md.starts_with("# Test – v2\n"), "{md}");
    assert!(md.contains("## Verdict") && md.contains("## Streaming") && md.contains("| Spotify |"), "{md}");
    assert!(export("notes.Markdown", ExportFormat::Markdown).unwrap().ends_with("notes.Markdown"));
    let pdf = export("report.md", ExportFormat::Pdf).unwrap();
    assert!(pdf.ends_with("report.md.pdf"), "{pdf}");
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
    assert!(export("report.PDF", ExportFormat::Pdf).unwrap().ends_with("report.PDF"));

    let err = export("no-such-dir/r.md", ExportFormat::Markdown).unwrap_err();
    assert!(err.starts_with("Couldn't save the report"), "{err}");
}
