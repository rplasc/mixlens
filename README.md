# MixLens

A Windows desktop app for checking mixes and masters before release. Drop in a bounce and MixLens measures loudness, peaks, stereo image, tonal balance and file edges in about a second, flags problems, compares versions sample by sample, and lets you ask an AI assistant about the results.

The assistant never hears or receives your audio. It reads the measurements MixLens computes and asks the analysis engine for more (a section's loudness, a version comparison, an encode test) when it needs them.

## What it checks

| Area | Measurements |
|---|---|
| Loudness | Integrated loudness, loudness range, short-term loudness over time (ITU-R BS.1770 / EBU R128), loudness of any section |
| Peaks | True peak (4× oversampled), sample peak per channel, samples near or over full scale, flat-topped clipping, crest factor, peak-to-loudness ratio |
| Streaming | Peaks after encoding to Ogg Vorbis q5/q9 and AAC 256/128 kbps (needs ffmpeg), rules for your loudness target |
| Stereo | Correlation overall and in 400 ms windows, correlation below 120 Hz, side/mid ratio, loudness change in mono, L/R balance |
| Tone | Third-octave balance against the song's fitted tilt, energy by octave, low-frequency peaks labelled with note names, 5–10 kHz peaks, sub-30 Hz energy |
| Edges | Silence before the first and after the last sound, tails cut off by the bounce range, fade-outs, silent gaps inside the song (a sign of muted or soloed tracks) |
| Format | Sample rate, bit depth, float vs integer, dither detection, DC offset |
| Versions | Sample-accurate alignment, null-test residual, which seconds changed, loudness change per section, tonal change per octave |

Findings are produced by deterministic rules, so they work offline and without an API key. Mark each version as a **mix** or a **master** and the rules adjust: mixes are checked for headroom and over-compression, masters against the streaming target and true-peak ceiling (−1 dBTP, or −2 dBTP when louder than the target).

Three optional settings sharpen the analysis:

- **Reference tracks.** Add a finished track you want the song to sit next to, under **References** in the sidebar (files with `ref` or `reference` in their name are recognised automatically). References aren't checked for problems. Comparing against one shows the level difference to match before listening, octave and third-octave balance, and differences in loudness, dynamics and width, instead of running a null test.
- **Genre.** Pick the song's genre to compare its octave balance, tilt, dynamics and low-end width with typical values for that style, and to adjust rule thresholds such as over-compression. The built-in presets are rough starting points; a reference track is a better guide.
- **Key.** Set the song's key and bass peaks are labelled as root, fifth or other scale notes, or as out of key or between notes. Prominent off-key peaks are flagged as likely resonances, and low-mid build-ups on the root or fifth are treated as partly musical. The Tone tab also shows the song's tuning offset from A=440 and the root and fifth frequencies that are safe to boost.

## Install

1. Open the [Releases](../../releases) page and download `MixLens_x.y.z_x64-setup.exe`.
2. Run it. The installer isn't code-signed yet, so Windows SmartScreen may show a warning: choose **More info → Run anyway**.
3. Optional, for the streaming encode check: install ffmpeg with `winget install Gyan.FFmpeg`, then restart MixLens.

The installer downloads Microsoft's WebView2 runtime if it's missing (it's already on Windows 11).

## Use

1. Drop one or more audio files (WAV, FLAC, AIFF, MP3, M4A) onto the window. Each file becomes a version of a song.
2. Read the **Overview**. Click a section of the loudness graph to measure just that part.
3. Open **Compare versions** to see exactly what changed between two bounces.
4. Ask the **Assistant**, for example "Is this mix ready to be mastered?" or "What changed since the previous version?"

### Assistant setup

Open **Settings** and pick a provider:

- **Anthropic**: paste an API key from console.anthropic.com. The default model is `claude-sonnet-5-5`; choose **Load models** to pick another.
- **OpenAI**: paste an API key, then choose **Load models** and pick a chat model.
- **Ollama (offline)**: run [Ollama](https://ollama.com) locally, pull a model that supports tool calling, then choose **Load models**. Nothing leaves your computer.

API keys are stored in Windows Credential Manager, not in MixLens's database, and are never sent to the interface layer; only measurements go to the provider you choose.

Advanced: set `MIXLENS_ANTHROPIC_BASE_URL` or `MIXLENS_OPENAI_BASE_URL` to route requests through a proxy or compatible gateway.

## Data

Projects, versions and analysis reports are stored in `%APPDATA%\app.mixlens.desktop\mixlens.db`. Audio files stay where they are; MixLens stores their path and a hash, and re-reads them only for comparisons, section measurements and encode checks. If a file changes after it was added, MixLens asks you to add it again rather than mixing up results.

## Development

Requirements: Rust (stable), Node.js 22, and on Windows the [Tauri prerequisites](https://tauri.app/start/prerequisites/) (WebView2 and the MSVC build tools).

```sh
npm install
npm install --prefix ui
npm run dev          # desktop app with hot reload
npm run build        # release installer in target/release/bundle
cargo test --workspace
```

The analysis engine also ships as a command-line tool:

```sh
cargo run --release -p mixlens-cli -- analyze song_mix.wav
cargo run --release -p mixlens-cli -- analyze song_master.wav --master --target -14
cargo run --release -p mixlens-cli -- analyze song_mix.wav --genre hiphop --key "F minor"
cargo run --release -p mixlens-cli -- compare song_master_v11.wav song_master_v12.wav
cargo run --release -p mixlens-cli -- compare song_master.wav reference.wav --reference
cargo run --release -p mixlens-cli -- presets
cargo run --release -p mixlens-cli -- section song_master.wav 172 198
cargo run --release -p mixlens-cli -- codec song_master.wav
```

Add `--ai` to `analyze` or `compare` to see the compact JSON the assistant receives.

### Layout

```text
crates/mixlens-core   Analysis engine: decoding, loudness, peaks, stereo, spectrum, edges, rules, comparison, encode check
crates/mixlens-cli    Command-line front end for the engine
src-tauri             Desktop backend: SQLite storage, settings, Tauri commands, AI agent and provider adapters
ui                    Angular interface
```

The engine decodes each file once and runs its analyzers in parallel. Reports keep 100 ms K-weighted energy blocks, so section loudness is computed later without decoding the audio again. The two most recently used files stay decoded in memory for comparisons.

### Releasing

Pushing to `main` runs the tests and builds a Windows installer as a downloadable workflow artifact. To publish a release, bump the version in `Cargo.toml`, `src-tauri/tauri.conf.json` and `package.json`, then push a tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The Release workflow builds the NSIS and MSI installers on Windows and attaches them to a GitHub Release.
