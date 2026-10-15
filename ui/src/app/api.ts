// Typed bridge to the Rust backend. Field names mirror the Rust structs (snake_case);
// command argument names are camelCase (Tauri converts them).

import { invoke } from '@tauri-apps/api/core';

export type Kind = 'mix' | 'master' | 'reference';
export type Severity = 'ok' | 'info' | 'warn' | 'error';
export type Provider = 'anthropic' | 'openai' | 'ollama';
export type ExportFormat = 'pdf' | 'markdown';

export interface MusicalKey { tonic: number; mode: 'major' | 'minor' }
export interface Project { id: number; name: string; version_count: number; created_at: string; genre: string | null; key: MusicalKey | null }

export interface GenrePreset {
  id: string; name: string; builtin: boolean; octave_target_db: number[]; octave_tolerance_db: number[];
  tilt_range: [number, number]; master_lufs_range: [number, number]; master_plr_range: [number, number];
  lra_range: [number, number]; mix_plr_min: number; low_corr_min: number; sibilance_p99_max: number;
}
export interface GenreMatch {
  preset_id: string; preset_name: string;
  bands: { lo: number; hi: number; share_db: number; target_db: number; delta_db: number; tolerance_db: number }[];
  checks: { metric: string; label: string; value: number; lo: number | null; hi: number | null; unit: string; within: boolean }[];
}
export type KeyRole = 'root' | 'fifth' | 'third' | 'scale_tone' | 'out_of_key' | 'between_notes';
export interface KeyedPeak { hz: number; note: string; cents: number; above_median_db: number; role: KeyRole }
export interface KeyAnalysis {
  key: MusicalKey; key_name: string; tuning_offset_cents: number | null; a4_hz: number;
  peaks: KeyedPeak[]; mid_peaks: KeyedPeak[]; safe_freqs: { note: string; hz: number; role: KeyRole }[];
}

export interface VersionSummary {
  id: number; project_id: number; label: string; file_name: string; path: string; kind: Kind;
  /** A reference's own genre preset id and key; other versions use the song's. */
  genre: string | null; key: MusicalKey | null;
  created_at: string; integrated_lufs: number; true_peak_dbtp: number; lra_lu: number;
  duration_s: number; sample_rate: number; sample_format: string; worst: Severity;
  errors: number; warnings: number;
}

export interface Finding { severity: Severity; code: string; title: string; detail: string }
export interface BandDev { fc: number; level_db: number; dev_db: number }
export interface Section { start_s: number; end_s: number; kind: 'silence' | 'quiet' | 'loud'; lufs: number }

export interface Report {
  schema: number;
  file: {
    path: string; file_name: string; sample_rate: number; channels: number;
    bits_per_sample: number | null; sample_format: string; duration_s: number; size_bytes: number; hash: string;
  };
  loudness: {
    integrated_lufs: number; lra_lu: number; true_peak_dbtp: number; sample_peak_dbfs: number;
    sample_peak_l_dbfs: number; sample_peak_r_dbfs: number; rms_dbfs: number; crest_db: number; plr_db: number;
    max_short_term_lufs: number; max_momentary_lufs: number; samples_near_full_scale: number;
    samples_over_full_scale: number; flat_top_runs: number; dc_offset: [number, number]; lr_balance_db: number;
    short_term: number[]; short_term_hop_s: number;
  };
  stereo: {
    correlation: number; side_mid_db: number; low_correlation: number; low_side_mid_db: number;
    mono_fold_lufs: number; mono_loss_lu: number; window_corr_min: number; window_corr_p5: number;
    window_corr_median: number; negative_window_fraction: number;
    low_corr_windows: { t: number; corr: number; level_dbfs: number }[];
  };
  spectrum: {
    tilt_db_per_oct: number; third_octave: BandDev[];
    octave_share: { lo: number; hi: number; share_db: number }[];
    lf_peaks: { hz: number; above_median_db: number; note: string }[];
    mid_peaks?: { hz: number; above_median_db: number; note: string }[];
    sub30_share_db: number;
    sibilance: { median_db: number; p99_db: number; max_db: number };
  };
  edges: {
    first_sound_s: number; last_sound_s: number; leading_silence_s: number; trailing_silence_s: number;
    tail_last50ms_dbfs: number; ends_on_silence: boolean; tail_truncated: boolean; fade_out_detected: boolean;
    internal_silences: { start_s: number; end_s: number }[];
  };
  sections: Section[];
  dither: null | { bits: number; samples_checked: number; distinct_codes: number; nonzero_fraction: number; level_dbfs: number; detected: boolean; note: string };
  findings: Finding[];
  genre_match?: GenreMatch | null;
  key_analysis?: KeyAnalysis | null;
  analysis_ms: number;
}

export interface ReportEnvelope { id: number; label: string; kind: Kind; path: string; report: Report }

export interface Comparison {
  a_file: string; b_file: string; resampled: boolean; lag_ms: number; gain_db: number;
  residual_rel_db: number; residual_dbfs: number; window_s: number; windows: number[];
  changed_regions: { start_s: number; end_s: number; mean_diff_rel_db: number; lufs_a: number; lufs_b: number; octave_delta_db: number[] }[];
  section_deltas: { start_s: number; end_s: number; kind: Section['kind']; lufs_a: number; lufs_b: number; delta_lu: number }[];
  metric_deltas: { name: string; a: number; b: number; delta: number; unit: string }[];
  third_octave_delta: { fc: number; dev_a: number; dev_b: number }[];
  octave_bands: string[];
}

export interface ReferenceComparison {
  version_file: string; reference_file: string; version_id: number; reference_id: number; level_match_db: number;
  version_key: string | null; reference_key: string | null; version_genre: string | null; reference_genre: string | null;
  octave: { lo: number; hi: number; version_db: number; reference_db: number; delta_db: number }[];
  third_octave_delta: BandDev[];
  /** a = reference, b = version. */
  metric_deltas: { name: string; a: number; b: number; delta: number; unit: string }[];
}

export interface SectionAnalysis {
  loudness: { start_s: number; end_s: number; lufs: number; vs_integrated_lu: number };
  bands: { start_s: number; end_s: number; octave_level_db: number[]; tilt_db_per_oct: number; third_octave: BandDev[] };
  octave_bands: string[];
}

export type Normalization = 'up_and_down' | 'down_only';
export interface Platform {
  id: string; name: string; target_lufs: number; normalization: Normalization; boost_ceiling_dbtp: number | null;
  ceiling_dbtp: number; loud_ceiling_dbtp: number | null; modes: { name: string; target_lufs: number; limiter: boolean }[];
  codecs: string[]; hires: boolean; confidence: 'documented' | 'reported'; note: string;
}
export interface Playback {
  target_lufs: number; wanted_gain_db: number; gain_db: number; playback_lufs: number;
  quieter_by_db: number; limiter_db: number; capped_by_peaks: boolean;
}
export interface PlatformResult {
  id: string; name: string; confidence: 'documented' | 'reported'; note: string; normalization: Normalization; hires: boolean;
  ceiling_dbtp: number; playback: Playback; modes: { name: string; playback: Playback }[];
  encoded_peak_dbtp: number | null; peak_after_dbtp: number; summary: string; checks: Finding[]; status: Severity;
}
export interface StreamingReport { kind: Kind; encoded: boolean; general: Finding[]; platforms: PlatformResult[] }

export interface CodecResult { codec: string; label: string; true_peak_dbtp: number | null; integrated_lufs: number | null; error: string | null }

export interface Settings {
  provider: Provider; anthropic_model: string; openai_model: string; ollama_model: string;
  ollama_url: string; ffmpeg_path: string; main_platform: string; target_lufs: number; max_tool_rounds: number;
}
export interface SettingsView extends Settings {
  has_anthropic_key: boolean; has_openai_key: boolean; ffmpeg_found: boolean; keyring_ok: boolean;
  /** The main platform's loudness target, or the custom one. */
  effective_target_lufs: number;
}

export interface ChatMessage { id: number; role: 'user' | 'assistant'; content: string; created_at: string }

export const inTauri = (): boolean => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

type Mock = (cmd: string, args: Record<string, unknown>) => Promise<unknown>;
let mock: Mock | null = null;

async function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inTauri()) return invoke<T>(cmd, args);
  // Browser preview (ng serve): use local sample data so the UI can be developed without the shell.
  mock ??= (await import('./mock')).mockInvoke;
  return mock(cmd, args) as Promise<T>;
}

export const api = {
  listProjects: () => call<Project[]>('list_projects'),
  createProject: (name: string) => call<Project>('create_project', { name }),
  renameProject: (id: number, name: string) => call<void>('rename_project', { id, name }),
  deleteProject: (id: number) => call<void>('delete_project', { id }),
  listGenrePresets: () => call<GenrePreset[]>('list_genre_presets'),
  updateProjectContext: (id: number, genre: string | null, key: MusicalKey | null) => call<Project>('update_project_context', { id, genre, key }),
  listVersions: (projectId: number) => call<VersionSummary[]>('list_versions', { projectId }),
  addVersion: (projectId: number, path: string, kind?: Kind) => call<VersionSummary>('add_version', { projectId, path, kind: kind ?? null, label: null }),
  updateVersion: (id: number, label: string, kind: Kind) => call<VersionSummary>('update_version', { id, label, kind }),
  updateVersionContext: (id: number, genre: string | null, key: MusicalKey | null) => call<VersionSummary>('update_version_context', { id, genre, key }),
  deleteVersion: (id: number) => call<void>('delete_version', { id }),
  getReport: (id: number) => call<ReportEnvelope>('get_report', { id }),
  compare: (a: number, b: number) => call<Comparison>('compare_versions', { a, b }),
  compareReference: (a: number, b: number) => call<ReferenceComparison>('compare_reference', { a, b }),
  section: (id: number, startS: number, endS: number) => call<SectionAnalysis>('section_analysis', { id, startS, endS }),
  codecCheck: (id: number) => call<CodecResult[]>('codec_check', { id }),
  listPlatforms: () => call<Platform[]>('list_platforms'),
  streamingReport: (id: number) => call<StreamingReport>('streaming_report', { id }),
  /** Adds the format's extension if the path lacks it; returns the path written. */
  exportReport: (id: number, path: string, format: ExportFormat) => call<string>('export_report', { id, path, format }),
  getSettings: () => call<SettingsView>('get_settings'),
  saveSettings: (settings: Settings) => call<SettingsView>('save_settings', { settings }),
  setApiKey: (provider: Provider, key: string) => call<SettingsView>('set_api_key', { provider, key }),
  listModels: (provider: Provider) => call<string[]>('list_models', { provider }),
  getChat: (projectId: number) => call<ChatMessage[]>('get_chat', { projectId }),
  clearChat: (projectId: number) => call<void>('clear_chat', { projectId }),
  chat: (projectId: number, message: string, focus: number[]) => call<ChatMessage>('ai_chat', { projectId, message, focus }),
};

export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
