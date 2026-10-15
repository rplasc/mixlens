//! Shareable report for a client or label: verdict, loudness and peaks, streaming playback and
//! flagged findings. Built once as a [`ReportDoc`], then rendered as Markdown or (with the `pdf`
//! feature) a PDF.

use crate::key::Role;
use crate::report::{BandDev, Finding, Kind, Report, Severity, Targets};
use crate::streaming::{Confidence, StreamingReport};

/// What the report is about and what it was checked against.
#[derive(Clone, Debug)]
pub struct ExportContext {
    pub song: String,
    pub version: String,
    /// Date the version was analyzed, e.g. "2026-10-15".
    pub analyzed_on: String,
    pub app_version: String,
    /// The rules the findings were evaluated with (kind, platform, genre, key).
    pub targets: Targets,
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub severity: Severity,
    pub headline: String,
    pub detail: String,
}

/// One measurement: value, the target it's held to (empty when none applies) and its status.
#[derive(Clone, Debug)]
pub struct Row {
    pub label: String,
    pub value: String,
    pub target: String,
    pub status: Option<Severity>,
}

#[derive(Clone, Debug)]
pub struct PlatformRow {
    pub name: String,
    /// The platform doesn't publish its rules; the figures are commonly reported ones.
    pub reported: bool,
    pub change: String,
    pub plays_at: String,
    pub peak_after: String,
    pub status: Severity,
    pub summary: String,
}

/// A table both renderers draw as given.
#[derive(Clone, Debug, Default)]
pub struct Table {
    pub head: Vec<String>,
    /// Indexes of the numeric columns, which align right.
    pub right: Vec<usize>,
    pub rows: Vec<Vec<String>>,
}

/// The Tone view: overall readings, energy by octave and low-frequency peaks.
#[derive(Clone, Debug)]
pub struct Tone {
    pub readings: Table,
    /// Third-octave deviation from the fitted tilt, 30 Hz up (drawn as a chart in the PDF).
    pub balance: Vec<BandDev>,
    /// What the octave shares are compared with.
    pub octave_note: String,
    pub octaves: Table,
    /// How to read the peaks (and in which key).
    pub peak_note: String,
    /// Empty when there are no prominent peaks.
    pub peaks: Table,
}

#[derive(Clone, Debug)]
pub struct ReportDoc {
    pub title: String,
    /// Short lines under the title: file and format, then what it was checked against.
    pub meta: Vec<String>,
    /// None for a reference track, which isn't held to mix or master rules.
    pub verdict: Option<Verdict>,
    pub loudness: Vec<Row>,
    pub platforms: Vec<PlatformRow>,
    /// Clipping and delivery-format checks that apply on every platform.
    pub delivery: Vec<Finding>,
    pub encode_note: String,
    pub tone: Tone,
    /// Findings that need reading, most severe first. Empty for a reference.
    pub findings: Vec<Finding>,
    /// Titles of the checks that passed.
    pub passed: Vec<String>,
    pub footer: String,
}

/// Plain word for a severity, as the UI says it (`SPOKEN` in `ui/src/app/severity.ts`).
pub fn severity_word(s: Severity) -> &'static str {
    match s {
        Severity::Error => "Problem",
        Severity::Warn => "Check",
        Severity::Info => "Note",
        Severity::Ok => "Passed",
    }
}

/// Fixed decimals with a typographic minus, as `num` in `ui/src/app/fmt.ts`.
fn num(v: f64, digits: usize) -> String {
    if !v.is_finite() {
        return "–".into();
    }
    let s = format!("{v:.digits$}");
    let s = if s.parse::<f64>() == Ok(0.0) { format!("{:.digits$}", 0.0) } else { s };
    s.replace('-', "−")
}

/// Like `num` with an explicit plus for positive values.
fn signed(v: f64, digits: usize) -> String {
    let s = num(v, digits);
    // Negative values carry "−", which doesn't parse, so only positive non-zero ones match.
    if s.parse::<f64>().is_ok_and(|x| x > 0.0) {
        format!("+{s}")
    } else {
        s
    }
}

/// Typographic minus for negative numbers in prose from the rules engine ("-14.0" to "−14.0").
fn prose(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev: Option<char> = None;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        let starts_number = c == '-' && it.peek().is_some_and(|n| n.is_ascii_digit()) && prev.is_none_or(|p| p.is_whitespace() || p == '(');
        out.push(if starts_number { '−' } else { c });
        prev = Some(c);
    }
    out
}

/// "48 kHz, 24-bit", as `format` in `ui/src/app/fmt.ts`.
fn format_line(sample_format: &str, rate: u32) -> String {
    let depth = sample_format.trim_start_matches("int").trim_start_matches("float");
    let kind = if sample_format.starts_with("float") {
        format!("{depth}-bit float")
    } else if sample_format.starts_with("int") {
        format!("{depth}-bit")
    } else {
        sample_format.to_string()
    };
    let khz = if rate.is_multiple_of(1000) { format!("{}", rate / 1000) } else { format!("{:.1}", rate as f64 / 1000.0) };
    format!("{khz} kHz, {kind}")
}

fn duration(s: f64) -> String {
    let t = s.max(0.0).round() as u64;
    format!("{}:{:02}", t / 60, t % 60)
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Mix => "Mix",
        Kind::Master => "Master",
        Kind::Reference => "Reference",
    }
}

fn clean(f: &Finding) -> Finding {
    Finding { severity: f.severity, code: f.code.clone(), title: prose(&f.title), detail: prose(&f.detail) }
}

/// The findings code for a streaming check that says the same thing.
fn same_check(code: &str) -> &str {
    match code {
        "float" => "format",
        c => c,
    }
}

/// Worst severity among findings with these codes (`READING_CODES` in `ui/src/app/overview.ts`).
fn status_of(findings: &[Finding], codes: &[&str]) -> Option<Severity> {
    findings.iter().filter(|f| codes.contains(&f.code.as_str())).map(|f| f.severity).max()
}

/// One-line answer to "is this ready, and what do I fix first?". Mirrors `Overview.verdict` in
/// `ui/src/app/overview.ts`; keep the two in step. Worded for a reader other than the producer.
pub fn verdict(findings: &[Finding], kind: Kind, platform: Option<&str>) -> Option<Verdict> {
    if kind == Kind::Reference {
        return None;
    }
    let errors: Vec<&Finding> = findings.iter().filter(|f| f.severity == Severity::Error).collect();
    let warns: Vec<&Finding> = findings.iter().filter(|f| f.severity == Severity::Warn).collect();
    let ok = |code: &str| findings.iter().any(|f| f.code == code && f.severity == Severity::Ok);
    let goal = if kind == Kind::Master { "before release" } else { "before mastering" };
    let place = platform.map_or("for the target".to_string(), |p| format!("for {p}"));

    let top = errors.first().or(warns.first());
    // What already passed, so a list of problems doesn't read as "nothing works".
    let already = if top.is_some() { "already " } else { "" };
    let passed = if kind == Kind::Master {
        if ok("loudness") && ok("tp_ceiling") {
            format!("Loudness and true peak are {already}on target {place}.")
        } else if ok("tp_ceiling") {
            format!("True peak is {already}under the ceiling {place}.")
        } else if ok("loudness") {
            format!("Loudness is {already}on target {place}.")
        } else {
            String::new()
        }
    } else if ok("mix_headroom") && !findings.iter().any(|f| f.code == "mix_overcompressed") {
        "Headroom and dynamics leave the master room to work.".into()
    } else {
        String::new()
    };

    let Some(top) = top else {
        return Some(Verdict { severity: Severity::Ok, headline: "No problems found".into(), detail: passed });
    };
    let n = if errors.is_empty() { warns.len() } else { errors.len() };
    let headline = if !errors.is_empty() {
        if n == 1 { format!("One problem to fix {goal}") } else { format!("{n} problems to fix {goal}") }
    } else if n == 1 {
        format!("One thing to check {goal}")
    } else {
        format!("{n} things to check {goal}")
    };
    let severity = if errors.is_empty() { Severity::Warn } else { Severity::Error };
    Some(Verdict { severity, headline, detail: format!("Start with: {}. {passed}", prose(&top.title)).trim().to_string() })
}

pub fn build(ctx: &ExportContext, r: &Report, s: &StreamingReport) -> ReportDoc {
    let t = &ctx.targets;
    let l = &r.loudness;
    let f = &r.findings;
    let master = t.kind == Kind::Master;
    let platform = t.platform.as_ref().map(|p| p.name.as_str());

    let mut meta = vec![format!(
        "{} · {} · {} · {}",
        kind_name(t.kind),
        r.file.file_name,
        format_line(&r.file.sample_format, r.file.sample_rate),
        duration(r.file.duration_s)
    )];
    let mut checked = vec![match (t.kind, platform) {
        (Kind::Reference, _) => "Reference track: shown for comparison, not checked against mix or master rules".to_string(),
        (Kind::Mix, _) => "Checked as a mix: headroom and dynamics before mastering".to_string(),
        (Kind::Master, Some(p)) => format!("Checked as a master for {p} ({} LUFS)", num(t.target_lufs, 0)),
        (Kind::Master, None) => format!("Checked as a master for a {} LUFS target", num(t.target_lufs, 0)),
    }];
    if let Some(g) = &t.genre {
        checked.push(format!("Genre: {}", g.name));
    }
    if let Some(k) = &t.key {
        checked.push(format!("Key: {}", k.name()));
    }
    meta.push(checked.join(" · "));

    // The stricter ceiling applies to masters louder than the target (`Platform::ceiling_for`).
    let ceiling = if l.integrated_lufs > t.target_lufs + 0.05 { t.loud_ceiling_dbtp } else { t.ceiling_dbtp };
    let row = |label: &str, value: String, target: String, codes: &[&str]| Row { label: label.into(), value, target, status: status_of(f, codes) };
    let loudness = vec![
        row(
            "Integrated loudness",
            format!("{} LUFS", num(l.integrated_lufs, 1)),
            if master { format!("{} LUFS", num(t.target_lufs, 0)) } else { String::new() },
            &["loudness", "loudness_low", "loudness_high"],
        ),
        row(
            "True peak",
            format!("{} dBTP", num(l.true_peak_dbtp, 1)),
            if master { format!("≤ {} dBTP", num(ceiling, 1)) } else { String::new() },
            &["tp_ceiling", "mix_headroom", "clipping", "over_full_scale"],
        ),
        row("Loudness range", format!("{} LU", num(l.lra_lu, 1)), String::new(), &[]),
        row("Peak to loudness (PLR)", format!("{} dB", num(l.plr_db, 1)), String::new(), &["mix_overcompressed"]),
        row("Sample peak", format!("{} dBFS", num(l.sample_peak_dbfs, 1)), String::new(), &[]),
        row("Max short-term loudness", format!("{} LUFS", num(l.max_short_term_lufs, 1)), String::new(), &[]),
    ];

    let platforms = s
        .platforms
        .iter()
        .map(|p| PlatformRow {
            name: p.name.clone(),
            reported: p.confidence == Confidence::Reported,
            change: if p.playback.gain_db.abs() < 0.05 { "none".into() } else { format!("{} dB", signed(p.playback.gain_db, 1)) },
            plays_at: format!("{} LUFS", num(p.playback.playback_lufs, 1)),
            peak_after: format!("{} dBTP", num(p.peak_after_dbtp, 1)),
            status: p.status,
            summary: prose(&p.summary),
        })
        .collect();

    let encode_note = if s.encoded {
        "Peaks after lossy encoding were measured for each platform's streams.".to_string()
    } else {
        "Peaks are measured before lossy encoding; encoded streams weren't checked.".to_string()
    };

    let reference = t.kind == Kind::Reference;
    let findings = if reference { vec![] } else { f.iter().filter(|x| x.severity != Severity::Ok).map(clean).collect() };
    let passed = if reference { vec![] } else { f.iter().filter(|x| x.severity == Severity::Ok).map(|x| prose(&x.title)).collect() };

    ReportDoc {
        title: format!("{} – {}", ctx.song, ctx.version),
        meta,
        verdict: verdict(f, t.kind, platform),
        loudness,
        platforms,
        // Clipping, dither and float checks are in the findings too: list them once, there.
        delivery: s.general.iter().filter(|g| !f.iter().any(|x| x.code == same_check(&g.code))).map(clean).collect(),
        encode_note,
        tone: tone(r),
        findings,
        passed,
        footer: format!("Analyzed {} · Report by MixLens {}", ctx.analyzed_on, ctx.app_version),
    }
}

/// Compact frequency, as `hz` in `ui/src/app/fmt.ts`: "80", "1.3k", "10k".
fn hz(f: f64) -> String {
    if f < 1000.0 {
        return format!("{}", f.round());
    }
    // `round` takes halves away from zero, as the UI does (1250 Hz reads "1.3k", not "1.2k").
    let k = if f >= 10000.0 { (f / 1000.0).round() } else { (f / 100.0).round() / 10.0 };
    format!("{k}k")
}

/// A typical range, as `Spectrum.range` in `ui/src/app/spectrum.ts`.
fn range(lo: Option<f64>, hi: Option<f64>, digits: usize) -> String {
    match (lo, hi) {
        (Some(lo), Some(hi)) => format!("{} to {}", num(lo, digits), num(hi, digits)),
        (Some(lo), None) => format!("at least {}", num(lo, digits)),
        (None, Some(hi)) => format!("at most {}", num(hi, digits)),
        (None, None) => String::new(),
    }
}

fn role_label(r: Role) -> &'static str {
    match r {
        Role::Root => "root",
        Role::Fifth => "fifth",
        Role::Third => "third",
        Role::ScaleTone => "in key",
        Role::OutOfKey => "out of key",
        Role::BetweenNotes => "between notes",
    }
}

fn status_cell(s: Option<Severity>) -> String {
    s.map_or("", severity_word).to_string()
}

fn strings(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

/// The Tone view (`ui/src/app/spectrum.ts`) as tables.
fn tone(r: &Report) -> Tone {
    let sp = &r.spectrum;
    let f = &r.findings;
    let genre = r.genre_match.as_ref();
    let check = |metric: &str| genre.and_then(|g| g.checks.iter().find(|c| c.metric == metric));
    let typical = |metric: &str, unit: &str| check(metric).map_or(String::new(), |c| format!("{} {unit}", range(c.lo, c.hi, 1)));

    let mut readings = vec![
        vec![
            "Tilt".to_string(),
            format!("{} dB/oct", num(sp.tilt_db_per_oct, 1)),
            if genre.is_some() { typical("tilt", "dB/oct") } else { "≈ −2 dB/oct for pop".into() },
            status_cell(status_of(f, &["genre_tilt"])),
        ],
        vec![
            "5–10 kHz vs full band, loudest 1%".into(),
            format!("{} dB", num(sp.sibilance.p99_db, 1)),
            typical("sibilance_p99", "dB"),
            status_cell(status_of(f, &["sibilance"])),
        ],
        vec!["5–10 kHz vs full band, typical".into(), format!("{} dB", num(sp.sibilance.median_db, 1)), String::new(), String::new()],
        vec!["Below 30 Hz, share of total".into(), format!("{} dB", num(sp.sub30_share_db, 1)), String::new(), status_cell(status_of(f, &["subsonic"]))],
    ];
    if let Some(k) = &r.key_analysis {
        let value = match k.tuning_offset_cents {
            Some(c) => format!("{} cents (A4 ≈ {} Hz)", signed(c, 0), num(k.a4_hz, 1)),
            None => "Too few peaks to tell".into(),
        };
        readings.push(vec!["Tuning".into(), value, String::new(), status_cell(status_of(f, &["tuning"]))]);
    }
    let readings = Table { head: strings(&["Measure", "Value", "Typical", "Status"]), right: vec![1, 2], rows: readings };

    let (octave_note, octaves) = match genre {
        Some(g) => {
            let rows = sp
                .octave_share
                .iter()
                .map(|o| {
                    let band = format!("{}–{} Hz", hz(o.lo), hz(o.hi));
                    match g.bands.iter().find(|b| b.lo == o.lo) {
                        Some(b) => vec![
                            band,
                            format!("{} dB", num(o.share_db, 1)),
                            format!("{} dB", num(b.target_db, 1)),
                            format!("{} dB", signed(b.delta_db, 1)),
                            status_cell((!b.within()).then_some(Severity::Warn)),
                        ],
                        None => vec![band, format!("{} dB", num(o.share_db, 1)), String::new(), String::new(), String::new()],
                    }
                })
                .collect();
            (
                format!("Each octave's share of the total energy, compared with a typical {} balance. These are broad averages; a reference track is a better guide.", g.preset_name),
                Table { head: strings(&["Band", "Share of total", "Typical", "Difference", "Status"]), right: vec![1, 2, 3], rows },
            )
        }
        None => (
            "Each octave's share of the total energy.".to_string(),
            Table {
                head: strings(&["Band", "Share of total"]),
                right: vec![1],
                rows: sp.octave_share.iter().map(|o| vec![format!("{}–{} Hz", hz(o.lo), hz(o.hi)), format!("{} dB", num(o.share_db, 1))]).collect(),
            },
        ),
    };

    let (peak_note, peaks) = match &r.key_analysis {
        Some(k) => (
            format!(
                "Narrow peaks between 30 and 300 Hz, read in {}. Peaks on notes in the key are usually the bassline or chords; peaks out of key or between notes are more likely resonances.",
                k.key_name
            ),
            Table {
                head: vec!["Frequency".into(), "Note".into(), format!("In {}", k.key_name), "Above median".into(), "Status".into()],
                right: vec![3],
                rows: k
                    .peaks
                    .iter()
                    .map(|p| {
                        let off_key = matches!(p.role, Role::OutOfKey | Role::BetweenNotes) && p.above_median_db >= 8.0;
                        vec![
                            format!("{} Hz", num(p.hz, 1)),
                            p.note.clone(),
                            role_label(p.role).into(),
                            format!("{} dB", signed(p.above_median_db, 1)),
                            status_cell(off_key.then_some(Severity::Warn)),
                        ]
                    })
                    .collect(),
            },
        ),
        None => (
            "Narrow peaks between 30 and 300 Hz. Without the song's key, bass notes and resonances can't be told apart.".to_string(),
            Table {
                head: strings(&["Frequency", "Note", "Above median"]),
                right: vec![2],
                rows: sp.lf_peaks.iter().map(|p| vec![format!("{} Hz", num(p.hz, 1)), p.note.clone(), format!("{} dB", signed(p.above_median_db, 1))]).collect(),
            },
        ),
    };

    // The UI's band chart starts at 30 Hz too.
    let balance = sp.third_octave.iter().filter(|b| b.fc >= 30.0).cloned().collect();
    Tone { readings, balance, octave_note, octaves, peak_note, peaks }
}

/// Platforms that don't publish their rules are marked with this.
const REPORTED_MARK: &str = "*";
const REPORTED_NOTE: &str = "* The platform doesn't publish its rules; these are commonly reported figures.";

/// Pipe-table cell: no pipes or line breaks.
fn cell(s: &str) -> String {
    s.replace('|', "/").replace('\n', " ")
}

fn md_table(t: &Table) -> String {
    let line = |cells: &[String]| format!("| {} |\n", cells.iter().map(|c| cell(c)).collect::<Vec<_>>().join(" | "));
    let align: Vec<String> = (0..t.head.len()).map(|i| if t.right.contains(&i) { "--:" } else { "---" }.to_string()).collect();
    let mut md = line(&t.head) + &format!("|{}|\n", align.join("|"));
    for r in &t.rows {
        md += &line(r);
    }
    md
}

impl ReportDoc {
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# {}\n\n{}\n", self.title, self.meta.join("  \n"));

        if let Some(v) = &self.verdict {
            md += &format!("\n## Verdict\n\n**{}.** {}\n", v.headline, v.detail);
        }

        md += "\n## Loudness and peaks\n\n| Measure | Value | Target | Status |\n|---|--:|--:|---|\n";
        for r in &self.loudness {
            let status = r.status.map_or("", severity_word);
            md += &format!("| {} | {} | {} | {} |\n", cell(&r.label), r.value, r.target, status);
        }

        md += "\n## Streaming\n\n| Platform | Volume change | Plays at | Peak after | Status |\n|---|--:|--:|--:|---|\n";
        for p in &self.platforms {
            let mark = if p.reported { REPORTED_MARK } else { "" };
            md += &format!("| {}{} | {} | {} | {} | {} |\n", cell(&p.name), mark, p.change, p.plays_at, p.peak_after, severity_word(p.status));
        }
        md += "\n";
        for p in &self.platforms {
            md += &format!("- **{}:** {}\n", p.name, p.summary);
        }
        if self.platforms.iter().any(|p| p.reported) {
            md += &format!("\n{}  \n{}\n", REPORTED_NOTE.replacen('*', "\\*", 1), self.encode_note);
        } else {
            md += &format!("\n{}\n", self.encode_note);
        }
        if !self.delivery.is_empty() {
            md += "\n### Clipping and delivery\n\n";
            for d in &self.delivery {
                md += &format!("- **{}: {}.** {}\n", severity_word(d.severity), d.title, d.detail);
            }
        }

        let t = &self.tone;
        md += &format!("\n## Tone\n\n{}", md_table(&t.readings));
        md += &format!("\n### Energy by octave\n\n{}\n\n{}", t.octave_note, md_table(&t.octaves));
        md += &format!("\n### Low-frequency peaks\n\n{}\n\n", t.peak_note);
        md += &if t.peaks.rows.is_empty() { "No prominent peaks.\n".to_string() } else { md_table(&t.peaks) };

        if self.verdict.is_some() {
            md += "\n## Findings\n";
            if self.findings.is_empty() {
                md += "\nNothing flagged.\n";
            }
            for (sev, heading) in [(Severity::Error, "Problems"), (Severity::Warn, "Checks"), (Severity::Info, "Notes")] {
                let group: Vec<&Finding> = self.findings.iter().filter(|f| f.severity == sev).collect();
                if group.is_empty() {
                    continue;
                }
                md += &format!("\n### {heading}\n\n");
                for f in group {
                    md += &format!("- **{}.** {}\n", f.title, f.detail);
                }
            }
            if !self.passed.is_empty() {
                md += &format!("\nPassed: {}.\n", self.passed.join(", "));
            }
        }

        md += &format!("\n---\n\n{}\n", self.footer);
        md
    }

    /// A4 PDF with the same sections as the Markdown. Severity is spelled out (or shown by the
    /// marker's shape) and coloured as in the app; the colour only repeats the word, so the page
    /// still reads correctly printed in greyscale.
    #[cfg(feature = "pdf")]
    pub fn to_pdf(&self) -> crate::Result<Vec<u8>> {
        pdf::render(self).map_err(|e| crate::Error::Export(e.to_string()))
    }
}

#[cfg(feature = "pdf")]
mod pdf {
    use super::{severity_word, ReportDoc, Table, REPORTED_MARK, REPORTED_NOTE};
    use crate::report::{Finding, Severity};
    use genpdf::elements::{Break, LinearLayout, Paragraph, TableLayout};
    use genpdf::error::Error;
    use genpdf::fonts::{FontData, FontFamily};
    use genpdf::style::{Color, Style};
    use genpdf::{render, Alignment, Context, Element, Margins, Position, RenderResult, Size};
    use printpdf::BuiltinFont;

    static REGULAR: &[u8] = include_bytes!("../fonts/IBMPlexSans-Regular.ttf");
    static BOLD: &[u8] = include_bytes!("../fonts/IBMPlexSans-Bold.ttf");

    const INK: Color = Color::Greyscale(20);
    const MUTED: Color = Color::Greyscale(95);
    const RULE: Color = Color::Greyscale(190);

    // The app's meaning colours (`ui/src/styles.css`) on white paper: the same hues, darkened
    // until text reaches 4.5:1 against white (bars 3:1). Colour only ever repeats what a word or
    // shape already says, so a greyscale print loses nothing.
    const ERR: Color = Color::Rgb(0xdf, 0x2c, 0x23);
    const WARN: Color = Color::Rgb(0xaf, 0x61, 0x0f);
    const OK: Color = Color::Rgb(0x3a, 0x84, 0x42);
    const INFO: Color = Color::Rgb(0x54, 0x78, 0x96);
    /// Measurement series (`--a`). Lighter than WARN in greyscale, so flagged bars still stand out.
    const GOLD: Color = Color::Rgb(0xbb, 0x8c, 0x1c);

    fn sev_color(s: Severity) -> Color {
        match s {
            Severity::Error => ERR,
            Severity::Warn => WARN,
            Severity::Info => INFO,
            Severity::Ok => OK,
        }
    }

    /// Colour for a status word in a table cell ("Problem", "Check", …).
    fn word_color(word: &str) -> Option<Color> {
        [Severity::Error, Severity::Warn, Severity::Info, Severity::Ok].into_iter().find(|&s| severity_word(s) == word).map(sev_color)
    }

    /// The UI's severity marker (`ui/src/app/severity.ts`): diamond = problem, triangle = check,
    /// ring = note, tick = passed. Drawn on its 10-unit grid, scaled to `SIZE`; solid shapes are
    /// filled with horizontal strokes, since genpdf only strokes lines.
    struct SevMark(Severity);

    /// A solid shape's (centre, half-width) at height y in grid units, or None outside it.
    type Span = fn(f64) -> Option<(f64, f64)>;

    impl SevMark {
        const SIZE: f64 = 2.8;
        /// Drops the mark to the middle of the headline, as `align-items: center` does in the UI.
        const TOP: f64 = 2.2;
    }

    impl Element for SevMark {
        fn render(&mut self, _: &Context, area: render::Area<'_>, style: Style) -> Result<RenderResult, Error> {
            let k = Self::SIZE / 10.0;
            let pt = |x: f64, y: f64| Position::new(x * k, Self::TOP + y * k);
            let style = style.with_color(sev_color(self.0));
            let solid: Option<Span> = match self.0 {
                Severity::Error => Some(|y| (0.4..=9.6).contains(&y).then(|| (5.0, 4.6 - (y - 5.0).abs()))),
                Severity::Warn => Some(|y| (0.6..=9.3).contains(&y).then(|| (5.0, (y - 0.6) / 8.7 * 4.7))),
                _ => None,
            };
            if let Some(half) = solid {
                let mut y = 0.0;
                while y <= 10.0 {
                    if let Some((cx, w)) = half(y) {
                        area.draw_line(vec![pt(cx - w, y), pt(cx + w, y)], style);
                    }
                    // Well under a stroke width (1 pt is 1.25 grid units here), so the edges stay smooth.
                    y += 0.35;
                }
            } else if self.0 == Severity::Info {
                let ring = (0..=32).map(|i| {
                    let a = i as f64 / 32.0 * std::f64::consts::TAU;
                    pt(5.0 + 3.7 * a.cos(), 5.0 + 3.7 * a.sin())
                });
                area.draw_line(ring.collect(), style);
            } else {
                area.draw_line(vec![pt(1.2, 5.3), pt(3.9, 8.0), pt(8.8, 2.2)], style);
            }
            Ok(RenderResult { size: Size::new(Self::SIZE, Self::TOP + Self::SIZE), has_more: false })
        }
    }

    fn family() -> Result<FontFamily<FontData>, Error> {
        // The report has no italics. Every slot is embedded, so the italic ones point at builtin
        // PDF fonts (nothing to embed) instead of a second copy of Plex.
        let font = |data: &[u8], builtin| FontData::new(data.to_vec(), builtin);
        Ok(FontFamily {
            regular: font(REGULAR, None)?,
            bold: font(BOLD, None)?,
            italic: font(REGULAR, Some(BuiltinFont::HelveticaOblique))?,
            bold_italic: font(BOLD, Some(BuiltinFont::HelveticaBoldOblique))?,
        })
    }

    /// Thin full-width rule.
    struct Rule;

    impl Element for Rule {
        fn render(&mut self, _: &Context, area: render::Area<'_>, style: Style) -> Result<RenderResult, Error> {
            let w = area.size().width;
            area.draw_line(vec![Position::new(0, 1), Position::new(w, 1)], style.with_color(RULE));
            Ok(RenderResult { size: Size::new(w, 2), has_more: false })
        }
    }

    /// Deviations past this (dB) between 150 Hz and 6 kHz are drawn darker (`flagAbove` in
    /// `ui/src/app/band-chart.ts`).
    const FLAG_DB: f64 = 3.5;

    /// Third-octave balance as bars up and down from the tilt line, after `BandChart` in
    /// `ui/src/app/band-chart.ts`. genpdf only strokes lines, so each bar is filled with
    /// closely spaced vertical strokes.
    struct BalanceChart {
        /// (centre frequency, deviation dB)
        bands: Vec<(f64, f64)>,
    }

    impl BalanceChart {
        /// Range shown either side of the tilt line (dB); larger deviations are clamped.
        const LIM: f64 = 9.0;
        /// Gutter for the dB labels, plot height and the frequency-label row (mm).
        const LEFT: f64 = 9.0;
        const PLOT_H: f64 = 40.0;
        const LABEL_H: f64 = 6.0;
        /// Stroke pitch inside a bar (mm); a 1 pt stroke is about 0.35 mm wide.
        const PITCH: f64 = 0.3;
        const HEIGHT: f64 = Self::PLOT_H + Self::LABEL_H;

        fn y(db: f64) -> f64 {
            let c = db.clamp(-Self::LIM, Self::LIM);
            (Self::LIM - c) / (2.0 * Self::LIM) * Self::PLOT_H
        }
    }

    impl Element for BalanceChart {
        fn render(&mut self, context: &Context, area: render::Area<'_>, style: Style) -> Result<RenderResult, Error> {
            let height = Self::HEIGHT;
            if f64::from(area.size().height) < height {
                // Start on the next page rather than splitting the chart.
                return Ok(RenderResult { size: Size::new(0, 0), has_more: true });
            }
            let width: f64 = area.size().width.into();
            let label = style.with_font_size(7).with_color(MUTED);
            let half_line = f64::from(label.line_height(&context.font_cache)) / 2.0;
            let line = |x0: f64, y0: f64, x1: f64, y1: f64, color| {
                area.draw_line(vec![Position::new(x0, y0), Position::new(x1, y1)], style.with_color(color));
            };

            for db in [6.0, 3.0, 0.0, -3.0, -6.0] {
                let y = Self::y(db);
                line(Self::LEFT, y, width, y, if db == 0.0 { MUTED } else { RULE });
                let text = if db == 0.0 { "0".to_string() } else { super::signed(db, 0) };
                let w = f64::from(label.str_width(&context.font_cache, &text));
                area.print_str(&context.font_cache, Position::new(Self::LEFT - 1.5 - w, y - half_line), label, &text)?;
            }

            let n = self.bands.len().max(1) as f64;
            let slot = (width - Self::LEFT) / n;
            let bw = slot * 0.62;
            let y0 = Self::y(0.0);
            for (i, &(fc, dev)) in self.bands.iter().enumerate() {
                let x0 = Self::LEFT + i as f64 * slot + (slot - bw) / 2.0;
                let mut y1 = Self::y(dev);
                if (y1 - y0).abs() < Self::PITCH {
                    y1 = y0 - Self::PITCH.copysign(dev);
                }
                let flag = (150.0..=6000.0).contains(&fc) && dev.abs() > FLAG_DB;
                let color = if flag { WARN } else { GOLD };
                // Strokes are centred on x, so inset by half a stroke to keep the bar's edges true.
                let mut x = x0 + 0.17;
                while x <= x0 + bw - 0.17 {
                    line(x, y0, x, y1, color);
                    x += Self::PITCH;
                }
            }

            for f in [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0] {
                let Some(i) = self.bands.iter().position(|&(fc, _)| (fc / f).log2().abs() < 0.17) else { continue };
                let text = super::hz(f);
                let w = f64::from(label.str_width(&context.font_cache, &text));
                let x = Self::LEFT + (i as f64 + 0.5) * slot - w / 2.0;
                area.print_str(&context.font_cache, Position::new(x, Self::PLOT_H + 1.5), label, &text)?;
            }

            Ok(RenderResult { size: Size::new(width, height), has_more: false })
        }
    }

    /// Starts a new page unless this much fits in what's left of the current one: body-text lines
    /// plus fixed millimetres. Draws nothing. Placed before a heading so the heading, or a table's
    /// header row, isn't stranded at the foot of a page away from what it introduces.
    #[derive(Clone, Copy)]
    struct KeepRoom {
        lines: f64,
        mm: f64,
    }

    impl KeepRoom {
        const fn lines(lines: f64) -> Self {
            KeepRoom { lines, mm: 0.0 }
        }
        fn plus(self, lines: f64) -> Self {
            KeepRoom { lines: self.lines + lines, ..self }
        }
    }

    /// The opening lines of a paragraph or finding.
    const TEXT_START: KeepRoom = KeepRoom::lines(3.0);
    /// A table's header row and its first three rows.
    const TABLE_START: KeepRoom = KeepRoom::lines(4.5);
    /// A two-line caption (8 pt) and the space after it.
    const CAPTION: f64 = 2.3;

    impl Element for KeepRoom {
        fn render(&mut self, context: &Context, area: render::Area<'_>, style: Style) -> Result<RenderResult, Error> {
            let need = self.lines * f64::from(style.line_height(&context.font_cache)) + self.mm;
            // On a fresh page there is always room (every use is far shorter than a page), so
            // this can't ask for page after page.
            Ok(RenderResult { size: Size::new(0, 0), has_more: f64::from(area.size().height) < need })
        }
    }

    /// Section heading with a rule; `next` is how much of what follows must fit beside it.
    fn heading(doc: &mut genpdf::Document, text: &str, next: KeepRoom) {
        // The guard goes after the space above, so a page break leaves that space on the old page.
        doc.push(Break::new(1.2));
        doc.push(next.plus(2.0));
        doc.push(Paragraph::new(text).styled(Style::new().bold().with_font_size(12).with_color(INK)));
        doc.push(Rule);
        doc.push(Break::new(0.3));
    }

    /// "Title. Detail" with the title in bold; `lead` puts the severity word, in its colour, first.
    fn finding(lead: bool, f: &Finding) -> impl Element {
        let mut p = Paragraph::default();
        if lead {
            p.push_styled(format!("{}: ", severity_word(f.severity)), Style::new().bold().with_color(sev_color(f.severity)));
        }
        p.push_styled(format!("{}. ", f.title), Style::new().bold());
        p.push(f.detail.clone());
        p.padded(Margins::trbl(0, 0, 1.5, 0))
    }

    fn table(weights: Vec<usize>, head: &[&str], rows: Vec<Vec<String>>, right: &[usize]) -> Result<TableLayout, Error> {
        let mut t = TableLayout::new(weights);
        let status = head.iter().position(|h| *h == "Status");
        let cell = |i: usize, s: &str, style: Style| {
            let align = if right.contains(&i) { Alignment::Right } else { Alignment::Left };
            Paragraph::new(s).aligned(align).styled(style).padded(Margins::trbl(0.4, 2, 0.4, 0))
        };
        let mut r = t.row();
        for (i, h) in head.iter().enumerate() {
            r.push_element(cell(i, h, Style::new().bold().with_font_size(8).with_color(MUTED)));
        }
        r.push()?;
        for row in rows {
            let mut r = t.row();
            for (i, s) in row.iter().enumerate() {
                let color = if Some(i) == status { word_color(s) } else { None };
                r.push_element(cell(i, s, color.map_or(Style::new(), |c| Style::new().with_color(c))));
            }
            r.push()?;
        }
        Ok(t)
    }

    /// A report table; with more weights than columns, the extra ones stay empty.
    fn grid(weights: Vec<usize>, t: &Table) -> Result<TableLayout, Error> {
        let n = weights.len();
        let pad = |cells: &[String]| {
            let mut c = cells.to_vec();
            c.resize(n.max(c.len()), String::new());
            c
        };
        let head = pad(&t.head);
        let head: Vec<&str> = head.iter().map(String::as_str).collect();
        table(weights, &head, t.rows.iter().map(|r| pad(r)).collect(), &t.right)
    }

    fn subheading(doc: &mut genpdf::Document, text: &str, next: KeepRoom) {
        subheading_after(doc, text, next, 0.8, INK);
    }

    /// Subheading after `space` lines of space; `next` is how much of what follows must fit beside it.
    fn subheading_after(doc: &mut genpdf::Document, text: &str, next: KeepRoom, space: f64, color: Color) {
        doc.push(Break::new(space));
        doc.push(next.plus(1.2));
        doc.push(Paragraph::new(text).styled(Style::new().bold().with_font_size(10).with_color(color)));
        doc.push(Break::new(0.2));
    }

    pub fn render(d: &ReportDoc) -> Result<Vec<u8>, Error> {
        let mut doc = genpdf::Document::new(family()?);
        doc.set_title(d.title.clone());
        doc.set_minimal_conformance();
        doc.set_font_size(10);
        doc.set_line_spacing(1.25);
        let mut deco = genpdf::SimplePageDecorator::new();
        deco.set_margins(Margins::trbl(18, 20, 18, 20));
        let running = d.title.clone();
        deco.set_header(move |page| {
            let mut l = LinearLayout::vertical();
            if page > 1 {
                l.push(Paragraph::new(format!("{running} · page {page}")).aligned(Alignment::Right).styled(Style::new().with_font_size(8).with_color(MUTED)));
                l.push(Break::new(1.0));
            }
            l
        });
        doc.set_page_decorator(deco);

        doc.push(Paragraph::new(d.title.as_str()).styled(Style::new().bold().with_font_size(18).with_color(INK)));
        doc.push(Break::new(0.3));
        for m in &d.meta {
            doc.push(Paragraph::new(m.as_str()).styled(Style::new().with_font_size(9).with_color(MUTED)));
        }

        if let Some(v) = &d.verdict {
            heading(&mut doc, "Verdict", TEXT_START);
            let mut text = LinearLayout::vertical();
            text.push(Paragraph::new(format!("{}.", v.headline)).styled(Style::new().bold().with_font_size(11)));
            if !v.detail.is_empty() {
                text.push(Paragraph::new(v.detail.as_str()));
            }
            // genpdf lays elements side by side only in tables: a narrow column holds the mark.
            let mut row = TableLayout::new(vec![2, 58]);
            row.row().element(SevMark(v.severity)).element(text).push()?;
            doc.push(row);
        }

        heading(&mut doc, "Loudness and peaks", TABLE_START);
        let rows = d.loudness.iter().map(|r| vec![r.label.clone(), r.value.clone(), r.target.clone(), r.status.map_or("", severity_word).to_string()]).collect();
        doc.push(table(vec![5, 3, 3, 2], &["Measure", "Value", "Target", "Status"], rows, &[1, 2])?);

        heading(&mut doc, "Streaming", TABLE_START);
        let rows = d
            .platforms
            .iter()
            .map(|p| {
                let mark = if p.reported { REPORTED_MARK } else { "" };
                vec![format!("{}{mark}", p.name), p.change.clone(), p.plays_at.clone(), p.peak_after.clone(), severity_word(p.status).to_string()]
            })
            .collect();
        doc.push(table(vec![4, 3, 3, 3, 2], &["Platform", "Volume change", "Plays at", "Peak after", "Status"], rows, &[1, 2, 3])?);
        doc.push(Break::new(0.6));
        for p in &d.platforms {
            let mut para = Paragraph::default();
            para.push_styled(format!("{}: ", p.name), Style::new().bold());
            para.push(p.summary.clone());
            doc.push(para.padded(Margins::trbl(0, 0, 1.2, 0)));
        }
        let note = Style::new().with_font_size(8).with_color(MUTED);
        if d.platforms.iter().any(|p| p.reported) {
            doc.push(Paragraph::new(REPORTED_NOTE).styled(note));
        }
        doc.push(Paragraph::new(d.encode_note.as_str()).styled(note));
        if !d.delivery.is_empty() {
            subheading(&mut doc, "Clipping and delivery", TEXT_START);
            for f in &d.delivery {
                doc.push(finding(true, f));
            }
        }

        let t = &d.tone;
        heading(&mut doc, "Tone", TABLE_START);
        doc.push(grid(vec![5, 3, 3, 2], &t.readings)?);
        if !t.balance.is_empty() {
            subheading(&mut doc, "Balance against the overall tilt", KeepRoom { lines: CAPTION, mm: BalanceChart::HEIGHT });
            let caption = format!(
                "Each bar is a third-octave band's distance from the song's fitted slope. Highlighted bars, in orange, are bumps or dips over {FLAG_DB} dB between 150 Hz and 6 kHz."
            );
            doc.push(Paragraph::new(caption).styled(note));
            doc.push(Break::new(0.3));
            doc.push(BalanceChart { bands: t.balance.iter().map(|b| (b.fc, b.dev_db)).collect() });
        }
        subheading(&mut doc, "Energy by octave", TABLE_START.plus(CAPTION));
        doc.push(Paragraph::new(t.octave_note.as_str()).styled(note));
        doc.push(Break::new(0.3));
        // Narrow tables keep their columns at the same widths; an empty column takes the rest.
        doc.push(grid(if t.octaves.head.len() > 2 { vec![4, 3, 3, 3, 2] } else { vec![4, 3, 8] }, &t.octaves)?);
        subheading(&mut doc, "Low-frequency peaks", TABLE_START.plus(CAPTION));
        doc.push(Paragraph::new(t.peak_note.as_str()).styled(note));
        doc.push(Break::new(0.3));
        if t.peaks.rows.is_empty() {
            doc.push(Paragraph::new("No prominent peaks."));
        } else {
            doc.push(grid(if t.peaks.head.len() > 3 { vec![3, 2, 3, 3, 2] } else { vec![3, 2, 3, 5] }, &t.peaks)?);
        }

        if d.verdict.is_some() {
            heading(&mut doc, "Findings", TEXT_START.plus(1.8));
            if d.findings.is_empty() {
                doc.push(Paragraph::new("Nothing flagged."));
            }
            for (sev, label) in [(Severity::Error, "Problems"), (Severity::Warn, "Checks"), (Severity::Info, "Notes")] {
                let group: Vec<&Finding> = d.findings.iter().filter(|f| f.severity == sev).collect();
                if group.is_empty() {
                    continue;
                }
                subheading_after(&mut doc, label, TEXT_START, 0.4, sev_color(sev));
                for f in group {
                    doc.push(finding(false, f));
                }
            }
            if !d.passed.is_empty() {
                doc.push(Break::new(0.4));
                let mut p = Paragraph::default();
                p.push_styled("Passed: ", Style::new().bold());
                p.push(format!("{}.", d.passed.join(", ")));
                doc.push(p.styled(Style::new().with_color(MUTED)));
            }
        }

        doc.push(Break::new(1.5));
        doc.push(Rule);
        doc.push(Paragraph::new(d.footer.as_str()).styled(Style::new().with_font_size(8).with_color(MUTED)));

        let mut out = Vec::new();
        doc.render(&mut out)?;
        Ok(out)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn status_words_take_their_severity_colour() {
            assert_eq!([("Problem", ERR), ("Check", WARN), ("Note", INFO), ("Passed", OK)].map(|(w, c)| word_color(w) == Some(c)), [true; 4]);
            // Anything else in a Status column stays ink.
            assert_eq!((word_color(""), word_color("Problems")), (None, None));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analyze, audio, builtin_platforms, refresh_findings, streaming};

    fn sine_report() -> Report {
        let fs = 48000;
        let x: Vec<f32> = (0..fs as usize * 10).map(|i| 0.9 * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin()).collect();
        let a = audio::from_planar(fs, x.clone(), x, "song_master.wav");
        analyze::analyze(&a, &Targets::default()).unwrap()
    }

    fn doc_for(kind: Kind) -> (Report, ReportDoc) {
        let mut r = sine_report();
        let spotify = builtin_platforms().into_iter().find(|p| p.id == "spotify").unwrap();
        let targets = Targets { kind, platform: Some(spotify), ..Targets::default() };
        refresh_findings(&mut r, &targets);
        let s = streaming::assess(&r, kind, &builtin_platforms(), None);
        let ctx = ExportContext { song: "Night Drive".into(), version: "Master v3".into(), analyzed_on: "2026-10-15".into(), app_version: "0.1.0".into(), targets };
        let d = build(&ctx, &r, &s);
        (r, d)
    }

    #[test]
    fn formats_like_the_ui() {
        assert_eq!(num(-14.0, 0), "−14");
        assert_eq!(num(-0.04, 1), "0.0");
        assert_eq!(signed(2.0, 1), "+2.0");
        assert_eq!(signed(-4.21, 1), "−4.2");
        assert_eq!(prose("-18.0 LUFS vs -14 LUFS (-1 dBTP), 20-40 Hz"), "−18.0 LUFS vs −14 LUFS (−1 dBTP), 20-40 Hz");
        assert_eq!(format_line("int24", 44100), "44.1 kHz, 24-bit");
        assert_eq!(format_line("float32", 48000), "48 kHz, 32-bit float");
        assert_eq!(duration(222.4), "3:42");
    }

    #[test]
    fn master_markdown_has_every_section() {
        let (r, d) = doc_for(Kind::Master);
        let md = d.to_markdown();
        assert!(md.starts_with("# Night Drive – Master v3\n"));
        let v = d.verdict.as_ref().unwrap();
        assert!(md.contains(&format!("**{}.**", v.headline)), "{md}");
        assert!(md.contains(&format!("| Integrated loudness | {} LUFS | −14 LUFS |", num(r.loudness.integrated_lufs, 1))), "{md}");
        for p in builtin_platforms() {
            assert!(md.contains(&format!("| {}", p.name)), "missing {}", p.name);
        }
        for f in r.findings.iter().filter(|f| f.severity != Severity::Ok) {
            assert!(md.contains(&prose(&f.title)), "missing finding {}", f.title);
        }
        assert!(md.contains("Checked as a master for Spotify (−14 LUFS)"));
        assert!(md.contains("Analyzed 2026-10-15"));
        // Every table row has as many cells as its header.
        let cells = |prefix: &str| md.lines().find(|l| l.starts_with(prefix)).unwrap().matches('|').count();
        assert_eq!(cells("| Integrated"), cells("| Measure"));
        assert_eq!(cells("| Spotify"), cells("| Platform"));
    }

    #[test]
    fn tone_follows_genre_and_key() {
        // Without song context: plain shares and peaks, no typical values.
        let (r, d) = doc_for(Kind::Master);
        let md = d.to_markdown();
        assert!(md.contains("## Tone") && md.contains(&format!("| Tilt | {} dB/oct |", num(r.spectrum.tilt_db_per_oct, 1))), "{md}");
        assert!(md.contains("| Band | Share of total |\n") && md.contains("| 80–160 Hz |"), "{md}");
        assert_eq!(d.tone.octaves.rows.len(), r.spectrum.octave_share.len());
        // The balance chart covers the same bands as the UI's, 30 Hz up.
        let charted = r.spectrum.third_octave.iter().filter(|b| b.fc >= 30.0).count();
        assert!(charted > 0 && d.tone.balance.len() == charted && d.tone.balance.iter().all(|b| b.fc >= 30.0));
        assert!(!md.contains("Tuning"));

        // With a genre and key: typical shares and differences, peaks read in the key, tuning.
        let mut r = sine_report();
        let pop = crate::builtin_presets().into_iter().find(|p| p.id == "pop").unwrap();
        let targets = Targets { kind: Kind::Master, genre: Some(pop), key: crate::MusicalKey::parse("A minor"), ..Targets::default() };
        refresh_findings(&mut r, &targets);
        let s = streaming::assess(&r, Kind::Master, &builtin_platforms(), None);
        let ctx = ExportContext { song: "S".into(), version: "V".into(), analyzed_on: String::new(), app_version: String::new(), targets };
        let d = build(&ctx, &r, &s);
        let md = d.to_markdown();
        assert!(md.contains("compared with a typical Pop balance") && md.contains("| Band | Share of total | Typical | Difference | Status |"), "{md}");
        assert!(md.contains("read in A minor") && md.contains("| Tuning |"), "{md}");
        let g = r.genre_match.as_ref().unwrap();
        let outside = g.bands.iter().filter(|b| !b.within()).count();
        assert_eq!(d.tone.octaves.rows.iter().filter(|row| row[4] == "Check").count(), outside);

        // Every table row has as many cells as its header.
        for t in [&d.tone.readings, &d.tone.octaves, &d.tone.peaks] {
            assert!(t.rows.iter().all(|row| row.len() == t.head.len()));
        }
    }

    #[test]
    fn frequencies_read_like_the_ui() {
        let got: Vec<String> = [80.0, 1250.0, 1280.0, 1000.0, 2500.0, 10240.0, 20480.0].map(hz).to_vec();
        assert_eq!(got, ["80", "1.3k", "1.3k", "1k", "2.5k", "10k", "20k"]);
        assert_eq!(range(Some(-2.8), Some(-1.4), 1), "−2.8 to −1.4");
        assert_eq!(range(None, Some(-6.0), 1), "at most −6.0");
    }

    #[test]
    fn verdict_matches_the_overview_wording() {
        let fin = |sev, code: &str, title: &str| Finding { severity: sev, code: code.into(), title: title.into(), detail: String::new() };
        let fs = vec![fin(Severity::Error, "tp_ceiling", "True peak above -1 dBTP"), fin(Severity::Ok, "loudness", "Loudness on target")];
        let v = verdict(&fs, Kind::Master, Some("Spotify")).unwrap();
        assert_eq!(v.headline, "One problem to fix before release");
        assert_eq!(v.detail, "Start with: True peak above −1 dBTP. Loudness is already on target for Spotify.");
        let v = verdict(&[fin(Severity::Ok, "mix_headroom", "Headroom")], Kind::Mix, None).unwrap();
        assert_eq!((v.severity, v.headline.as_str()), (Severity::Ok, "No problems found"));
        assert!(verdict(&fs, Kind::Reference, None).is_none());
    }

    #[test]
    fn reference_has_no_verdict_or_findings() {
        let (_, d) = doc_for(Kind::Reference);
        let md = d.to_markdown();
        assert!(!md.contains("## Verdict") && !md.contains("## Findings"), "{md}");
        assert!(md.contains("## Streaming") && md.contains("Reference track: shown for comparison"));
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn renders_a_pdf() {
        let (_, d) = doc_for(Kind::Master);
        let pdf = d.to_pdf().unwrap();
        assert!(pdf.starts_with(b"%PDF"));
        assert!(pdf.len() < 1_500_000, "{} bytes", pdf.len());
    }
}
