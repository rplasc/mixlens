//! AI mode: a small tool-using agent. The model never touches audio; it reads compact
//! reports and asks the DSP engine for more through tools.

pub mod providers;
pub mod tools;

use crate::db;
use crate::settings::{self, Provider};
use crate::state::Shared;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Clone, Debug)]
pub enum Msg {
    User(String),
    Assistant { text: String, calls: Vec<ToolCall> },
    Tool(Vec<ToolResult>),
}

pub struct Turn {
    pub text: String,
    pub calls: Vec<ToolCall>,
}

pub const SYSTEM_PROMPT: &str = r#"You are MixLens, a mixing and mastering analysis assistant inside a desktop app. You cannot hear audio. You see measurements from a deterministic DSP engine and can request more with tools.

Rules:
- Only cite numbers that appear in tool results or the provided context. Never invent measurements. If a question needs something that isn't measured (vocal level vs instruments, arrangement, taste), say so and say what to listen for.
- Call tools instead of guessing: get_report for a version's details, compare_versions to explain what changed between versions, section_analysis to inspect a time range (chorus, outro), codec_check before approving a master for streaming.
- Lead with the verdict, then the evidence (numbers, times, frequencies), then concrete next steps with values (e.g. "broad bell -1.5 dB at 420 Hz, Q about 1").
- Separate fixes that belong in the mix from moves that belong in mastering.
- Keep answers compact. Use a short table when comparing numbers across versions.

How to read the measurements:
- third_octave dev_db is deviation from a fitted tilt line: it shows bumps and dips, not absolute EQ. tilt_db_per_oct near -2 is typical for pop mixes.
- lf_peaks labelled with note names are usually the bassline or chords, not room or EQ resonances.
- sibilance values are 5-10 kHz energy relative to full band in the louder half of the song; p99 above about -6 dB deserves a listen.
- short-term loudness is 3 s windows; sections are derived from it (loud/quiet/silence).
- Comparisons: per-window diff near -120 dB means identical; above -20 dB means that region changed. Time-varying plugins (tape wobble, chorus, noise) prevent perfect nulls even when nothing was edited. gain_db is only meaningful when residual_rel_db is low.
- Internal digital silence or missing sections versus a previous version usually means tracks were muted or soloed during export.
- Versions marked [reference] are other artists' finished tracks the user wants to sound like. They get no mix/master findings. compare_versions against one returns mode "reference": there is no null test, and the user should level-match first (level_match_db). Treat differences as directions to listen in, not targets to copy, and keep genre and arrangement differences in mind. A reference has its own genre and key (reference_genre, reference_key), which can differ from the song's: when the keys differ, bass notes and low-frequency peaks sit at different pitches, so compare the shape and weight of the low end, not individual peaks.
- genre_match compares octave energy shares with the song's genre preset: delta_db beyond tolerance_db is unusual for the genre, and checks lists typical ranges. Presets are rough starting points, so a reference track outranks them.
- key_analysis interprets tonal peaks in the song's key: role root/fifth/third/in key are the music; out_of_key or between_notes peaks are likely room or EQ resonances or clashing notes. tuning_offset_cents is the song's tuning versus A=440. safe_freqs are root and fifth frequencies that suit bass boosts.
- tail_truncated means the file ends before the signal reaches silence: above -60 dBFS in the last 50 ms (tail_last50ms_dbfs) the ending audibly cuts off; below that only a faint tail is cut, which mastering gain can raise. Trailing silence counts toward track length.
- peak_marks lists where clipping (over_full_scale, clipping) and true peak over the ceiling (true_peak) happen: start_s/end_s and the highest true peak there. Cite those times so the user can find them in their DAW.
- Versions may carry notes: the user's own words about that version (what they changed, what they heard, often with times). Treat notes as claims to check, not measurements. For "what changed since my notes", run compare_versions from the noted version to the newer one and say which noted changes the measurements confirm, which they don't show, and what else changed.

Streaming: call streaming_check for how each platform turns the track up or down, how loud it then plays next to other tracks, and whether it meets ceilings; run codec_check first so it can use post-encode peaks. Spotify documents its rules (-14 LUFS; quieter masters only raised until the true peak reaches -1 dBTP; true peak <= -1 dBTP, or <= -2 dBTP when louder than -14 LUFS; Premium Loud -11 with a limiter, Quiet -19). The other platforms' figures (Apple Music ~-16, YouTube/Amazon/Tidal ~-14 turn-down only, Deezer ~-15) are reported by third parties, not published, so say so when it matters. Distributors commonly want 16- or 24-bit WAV at 44.1 kHz or higher; dither when reducing bit depth."#;

fn fmt_time(s: f64) -> String {
    format!("{}:{:04.1}", (s / 60.0).floor() as i64, s % 60.0)
}

/// Most characters of a version's notes sent with every question; get_report has them in full.
const NOTES_IN_CONTEXT: usize = 500;

/// A version's notes on one line, shortened for the per-turn context. `None` when empty.
fn notes_line(notes: &str) -> Option<String> {
    let one = notes.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" / ");
    if one.is_empty() {
        return None;
    }
    let mut short: String = one.chars().take(NOTES_IN_CONTEXT).collect();
    if short.len() < one.len() {
        short.push('…');
    }
    Some(short)
}

/// Compact per-version context, rebuilt every turn (cheap) so the system prompt stays cacheable.
pub(crate) fn project_context(st: &settings::Settings, project: Option<&db::Project>, presets: &[mixlens_core::GenrePreset], rows: &[db::VersionRow], focus: &[i64]) -> String {
    let genre = project.and_then(|p| p.genre.as_deref()).map(|id| presets.iter().find(|g| g.id == id).map_or(id, |g| g.name.as_str()));
    let key = project.and_then(|p| p.key).map(|k| k.name());
    let platform = match crate::state::main_platform(st) {
        Some(p) => format!("{} ({:.0} LUFS)", p.name, p.target_lufs),
        None => format!("custom target {:.1} LUFS", st.target_lufs),
    };
    let mut s = format!(
        "Main streaming platform: {platform}\nSong: genre {}, key {}\nProject versions (oldest first):\n",
        genre.unwrap_or("not set"),
        key.as_deref().unwrap_or("not set")
    );
    for v in rows {
        let r = &v.report;
        let issues: Vec<String> = r
            .findings
            .iter()
            .filter(|f| matches!(f.severity, mixlens_core::Severity::Warn | mixlens_core::Severity::Error))
            .map(|f| format!("{}({:?})", f.code, f.severity).to_lowercase())
            .collect();
        s.push_str(&format!(
            "- id {} \"{}\" [{}] {} | {} | {} Hz {} | {:.1} LUFS, {:.1} dBTP, LRA {:.1} | issues: {}\n",
            v.id,
            v.label,
            db::kind_str(v.kind),
            r.file.file_name,
            fmt_time(r.file.duration_s),
            r.file.sample_rate,
            r.file.sample_format,
            r.loudness.integrated_lufs,
            r.loudness.true_peak_dbtp,
            r.loudness.lra_lu,
            if issues.is_empty() { "none".into() } else { issues.join(", ") }
        ));
        if v.kind == mixlens_core::Kind::Reference {
            // References are other songs, with their own genre and key.
            let genre = v.genre.as_deref().map(|id| presets.iter().find(|g| g.id == id).map_or(id, |g| g.name.as_str()));
            let key = v.key.map(|k| k.name());
            s.pop();
            s.push_str(&format!(" | own genre {}, key {}\n", genre.unwrap_or("not set"), key.as_deref().unwrap_or("not set")));
        }
        if let Some(notes) = notes_line(&v.notes) {
            s.pop();
            s.push_str(&format!(" | notes: \"{notes}\"\n"));
        }
    }
    if !focus.is_empty() {
        s.push_str(&format!("The user is currently looking at version id(s): {:?}\n", focus));
    }
    s
}

/// The tool loop, independent of Tauri so it can be tested.
pub async fn converse(
    state: Shared,
    st: &settings::Settings,
    provider: Provider,
    model: &str,
    key: Option<&str>,
    mut msgs: Vec<Msg>,
    emit: impl Fn(Value),
) -> Result<String, String> {
    let defs = tools::definitions();
    let max_rounds = st.max_tool_rounds.clamp(1, 16);
    for round in 0..=max_rounds {
        let last = round == max_rounds;
        let call = providers::Call {
            provider,
            model,
            key,
            ollama_url: &st.ollama_url,
            system: SYSTEM_PROMPT,
            tools: if last { &[] } else { &defs },
            messages: &msgs,
        };
        emit(json!({"stage": "thinking", "round": round}));
        let turn = providers::chat(&state.http, &call).await?;
        if turn.calls.is_empty() {
            return Ok(turn.text);
        }
        let mut results = vec![];
        for tc in &turn.calls {
            emit(json!({"stage": "tool", "tool": tc.name, "args": tc.args}));
            let s2 = state.clone();
            let (name, args) = (tc.name.clone(), tc.args.clone());
            let out = tauri::async_runtime::spawn_blocking(move || tools::execute(&s2, &name, &args))
                .await
                .map_err(|e| e.to_string())?;
            let (content, is_error) = match out {
                Ok(v) => (v.to_string(), false),
                Err(e) => (json!({"error": e}).to_string(), true),
            };
            results.push(ToolResult { id: tc.id.clone(), name: tc.name.clone(), content, is_error });
        }
        msgs.push(Msg::Assistant { text: turn.text, calls: turn.calls });
        msgs.push(Msg::Tool(results));
    }
    Ok(String::new())
}

pub async fn run(state: Shared, app: AppHandle, project_id: i64, question: String, focus: Vec<i64>) -> Result<db::Message, String> {
    let st = state.settings();
    let provider = st.provider;
    let model = st.model_for(provider).to_string();
    let key = match provider {
        Provider::Ollama => None,
        p => settings::get_key(p),
    };

    let (rows, history, project, presets) = {
        let c = state.db.lock().unwrap();
        (
            db::list_versions(&c, project_id).map_err(|e| e.to_string())?,
            db::messages(&c, project_id).map_err(|e| e.to_string())?,
            db::get_project(&c, project_id).map_err(|e| e.to_string())?,
            crate::state::load_presets(&c),
        )
    };
    if rows.is_empty() {
        return Err("Add at least one version to this project first.".into());
    }

    // Recent conversation (text only) to keep requests small.
    let mut msgs: Vec<Msg> = vec![];
    for m in history.iter().rev().take(10).rev() {
        let next = if m.role == "user" { Msg::User(m.content.clone()) } else { Msg::Assistant { text: m.content.clone(), calls: vec![] } };
        // Keep strict user/assistant alternation for providers that require it.
        match (msgs.last_mut(), &next) {
            (Some(Msg::User(prev)), Msg::User(t)) => {
                prev.push_str("\n\n");
                prev.push_str(t);
            }
            (Some(Msg::Assistant { text: prev, .. }), Msg::Assistant { text: t, .. }) => {
                prev.push_str("\n\n");
                prev.push_str(t);
            }
            (None, Msg::Assistant { .. }) => {}
            _ => msgs.push(next),
        }
    }
    let user_turn = format!("{}\nQuestion: {}", project_context(&st, project.as_ref(), &presets, &rows, &focus), question);
    if let Some(Msg::User(prev)) = msgs.last_mut() {
        prev.push_str("\n\n");
        prev.push_str(&user_turn);
    } else {
        msgs.push(Msg::User(user_turn));
    }
    {
        let c = state.db.lock().unwrap();
        db::add_message(&c, project_id, "user", &question).map_err(|e| e.to_string())?;
    }

    let emit = |v: Value| {
        let mut v = v;
        v["project_id"] = json!(project_id);
        let _ = app.emit("ai-activity", v);
    };
    let mut answer = converse(state.clone(), &st, provider, &model, key.as_deref(), msgs, emit).await?;
    if answer.trim().is_empty() {
        answer = "I couldn't produce an answer. Try rephrasing, or check the model in Settings.".into();
    }
    let c = state.db.lock().unwrap();
    db::add_message(&c, project_id, "assistant", &answer).map_err(|e| e.to_string())
}
