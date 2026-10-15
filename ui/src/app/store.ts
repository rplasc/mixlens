import { Injectable, computed, effect, signal } from '@angular/core';
import {
  api, ChatMessage, CodecResult, Comparison, errorText, ExportFormat, GenrePreset, inTauri, Kind, MusicalKey, Platform, Project,
  Provider, ReferenceComparison, ReportEnvelope, SectionAnalysis, SettingsView, StreamingReport, VersionSummary,
} from './api';

export type Tab = 'overview' | 'spectrum' | 'stereo' | 'streaming' | 'compare';

/** Save-dialog file type per report format. */
const EXPORT_TYPES: Record<ExportFormat, { name: string; ext: string }> = {
  pdf: { name: 'PDF', ext: 'pdf' },
  markdown: { name: 'Markdown', ext: 'md' },
};

const TOOL_LABELS: Record<string, string> = {
  get_report: 'Reading the report',
  compare_versions: 'Comparing versions',
  section_analysis: 'Measuring a section',
  codec_check: 'Encoding test files',
  streaming_check: 'Checking streaming platforms',
};

/** One-time hints, remembered on this device. Storage can be unavailable; then a hint just shows again. */
function readFlag(key: string): boolean {
  try {
    return localStorage.getItem(key) === '1';
  } catch {
    return false;
  }
}
function writeFlag(key: string) {
  try {
    localStorage.setItem(key, '1');
  } catch {
    // Not stored: the hint reappears next launch, which is harmless.
  }
}

@Injectable({ providedIn: 'root' })
export class Store {
  readonly projects = signal<Project[]>([]);
  readonly projectId = signal<number | null>(null);
  readonly versions = signal<VersionSummary[]>([]);
  readonly versionId = signal<number | null>(null);
  readonly report = signal<ReportEnvelope | null>(null);
  readonly tab = signal<Tab>('overview');

  readonly compareWith = signal<number | null>(null);
  readonly comparison = signal<Comparison | null>(null);
  readonly refComparison = signal<ReferenceComparison | null>(null);
  readonly compareReport = signal<ReportEnvelope | null>(null);
  readonly comparing = signal(false);

  readonly codec = signal<CodecResult[] | null>(null);
  readonly codecBusy = signal(false);
  readonly streaming = signal<StreamingReport | null>(null);
  readonly platforms = signal<Platform[]>([]);
  readonly section = signal<SectionAnalysis | null>(null);
  readonly sectionIndex = signal<number | null>(null);
  /** The peak marker pinned on the loudness timeline (index into `peak_marks`). */
  readonly markIndex = signal<number | null>(null);

  readonly analyzing = signal<string[]>([]);
  readonly dragOver = signal(false);
  readonly error = signal<string | null>(null);
  readonly exportState = signal<'idle' | 'busy' | 'saved'>('idle');

  readonly settings = signal<SettingsView | null>(null);
  readonly presets = signal<GenrePreset[]>([]);
  readonly showSettings = signal(false);
  /** Provider picked in the assistant panel; Settings opens on it, then clears it. */
  readonly setupProvider = signal<Provider | null>(null);

  setUpAssistant(provider: Provider) {
    this.setupProvider.set(provider);
    this.showSettings.set(true);
  }

  readonly chat = signal<ChatMessage[]>([]);
  readonly chatBusy = signal(false);
  readonly activity = signal<string | null>(null);
  /** Closed until a provider is set up; see the constructor. */
  readonly chatOpen = signal(false);
  /** The first-analysis hint explaining mix vs master rules has been dismissed. */
  readonly kindTipSeen = signal(readFlag('mixlens.tip.kind'));
  /** The song list drawer, used only when the window is too narrow for the sidebar. */
  readonly navOpen = signal(false);

  /** Counts compare runs, so a result that arrives late can tell it was superseded. */
  private compareRun = 0;

  readonly project = computed(() => this.projects().find((p) => p.id === this.projectId()) ?? null);
  readonly version = computed(() => this.versions().find((v) => v.id === this.versionId()) ?? null);
  readonly otherVersions = computed(() => this.versions().filter((v) => v.id !== this.versionId()));
  readonly mixVersions = computed(() => this.versions().filter((v) => v.kind !== 'reference'));
  readonly references = computed(() => this.versions().filter((v) => v.kind === 'reference'));
  readonly genre = computed(() => this.presets().find((g) => g.id === this.project()?.genre) ?? null);
  /** Genre and key the header edits: a reference's own, otherwise the song's. */
  readonly context = computed(() => {
    const v = this.version();
    const src = v?.kind === 'reference' ? v : this.project();
    return { reference: v?.kind === 'reference', genre: src?.genre ?? null, key: src?.key ?? null };
  });
  /** The comparison in view is against a reference track rather than another bounce. */
  readonly comparingReference = computed(() => {
    const kind = (id: number | null) => this.versions().find((v) => v.id === id)?.kind;
    return kind(this.versionId()) === 'reference' || kind(this.compareWith()) === 'reference';
  });

  dismissKindTip() {
    this.kindTipSeen.set(true);
    writeFlag('mixlens.tip.kind');
  }

  /** The assistant has a provider and model it can use. */
  readonly assistantReady = computed(() => {
    const s = this.settings();
    if (!s) return false;
    if (s.provider === 'anthropic') return s.has_anthropic_key;
    if (s.provider === 'openai') return s.has_openai_key && !!s.openai_model;
    return !!s.ollama_model;
  });

  constructor() {
    // Open the assistant when it becomes usable: at launch, or right after a key is added in Settings.
    // In a narrow window it would cover the measurements, so it waits to be opened.
    let wasReady = false;
    effect(() => {
      const ready = this.assistantReady();
      if (ready && !wasReady && matchMedia('(min-width: 1000px)').matches) this.chatOpen.set(true);
      wasReady = ready;
    });
  }

  async init() {
    try {
      const [settings, presets, platforms, projects] = await Promise.all([
        api.getSettings(), api.listGenrePresets(), api.listPlatforms(), api.listProjects(),
      ]);
      this.settings.set(settings);
      this.presets.set(presets);
      this.platforms.set(platforms);
      this.projects.set(projects);
      if (projects.length) await this.selectProject(projects[0].id);
    } catch (e) {
      this.fail(e);
    }
    if (inTauri()) {
      const { listen } = await import('@tauri-apps/api/event');
      await listen<{ project_id: number; stage: string; tool?: string }>('ai-activity', (ev) => {
        if (ev.payload.project_id !== this.projectId()) return;
        this.activity.set(ev.payload.stage === 'tool' ? TOOL_LABELS[ev.payload.tool ?? ''] ?? 'Measuring' : 'Thinking');
      });
      const { getCurrentWebview } = await import('@tauri-apps/api/webview');
      await getCurrentWebview().onDragDropEvent((ev) => {
        const p = ev.payload;
        if (p.type === 'enter' || p.type === 'over') this.dragOver.set(true);
        else if (p.type === 'leave') this.dragOver.set(false);
        else if (p.type === 'drop') {
          this.dragOver.set(false);
          void this.addFiles(p.paths);
        }
      });
    }
  }

  fail(e: unknown) {
    this.error.set(errorText(e));
  }

  // ---------- projects ----------

  async refreshProjects() {
    this.projects.set(await api.listProjects());
  }

  async createProject(name: string) {
    try {
      const p = await api.createProject(name);
      await this.refreshProjects();
      await this.selectProject(p.id);
    } catch (e) {
      this.fail(e);
    }
  }

  async deleteProject() {
    const id = this.projectId();
    if (id == null) return;
    try {
      await api.deleteProject(id);
    } catch (e) {
      this.fail(e);
      return;
    }
    this.projectId.set(null);
    this.versions.set([]);
    this.versionId.set(null);
    this.report.set(null);
    this.chat.set([]);
    await this.refreshProjects();
    const first = this.projects()[0];
    if (first) await this.selectProject(first.id);
  }

  async selectProject(id: number) {
    this.projectId.set(id);
    this.versionId.set(null);
    this.report.set(null);
    this.resetDerived();
    try {
      const [versions, chat] = await Promise.all([api.listVersions(id), api.getChat(id)]);
      if (this.projectId() !== id) return;
      this.versions.set(versions);
      this.chat.set(chat);
      const last = versions.filter((v) => v.kind !== 'reference').at(-1) ?? versions.at(-1);
      if (last) await this.selectVersion(last.id);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Sets the selected reference's own genre and key, or the song's for any other version. */
  async setContext(genre: string | null, key: MusicalKey | null) {
    const v = this.version();
    if (v?.kind !== 'reference') return this.setSongContext(genre, key);
    try {
      const next = await api.updateVersionContext(v.id, genre, key);
      this.versions.update((list) => list.map((x) => (x.id === v.id ? next : x)));
      await this.reloadReport(v.id);
    } catch (e) {
      this.fail(e);
    }
  }

  async setSongContext(genre: string | null, key: MusicalKey | null) {
    const pid = this.projectId();
    if (pid == null) return;
    try {
      await api.updateProjectContext(pid, genre, key);
      const [projects, versions] = await Promise.all([api.listProjects(), api.listVersions(pid)]);
      this.projects.set(projects);
      if (this.projectId() !== pid) return;
      this.versions.set(versions);
      const id = this.versionId();
      if (id != null) await this.reloadReport(id);
    } catch (e) {
      this.fail(e);
    }
  }

  // ---------- versions ----------

  /** Reload a version's report after its type or context changed, and the comparison showing it. */
  private async reloadReport(id: number) {
    const report = await api.getReport(id);
    if (this.versionId() !== id) return;
    this.report.set(report);
    // Marks are re-derived for the new rules, so an index may point elsewhere now.
    this.markIndex.set(null);
    void this.locatePeaks(report);
    if (this.comparison() || this.refComparison()) await this.runCompare();
  }

  /** Versions analyzed before peak positions were recorded find them in the background. */
  private async locatePeaks(env: ReportEnvelope) {
    if (env.peaks_located !== false || env.kind === 'reference' || !inTauri()) return;
    try {
      const v = await api.locatePeaks(env.id);
      this.versions.update((list) => list.map((x) => (x.id === v.id ? v : x)));
      if (this.versionId() === env.id) await this.reloadReport(env.id);
    } catch {
      // The file moved or changed since it was added: the timeline just has no peak markers.
    }
  }

  /** The id is explicit, so a save fired on blur while switching versions lands on the right one. */
  async saveNotes(id: number, notes: string): Promise<boolean> {
    try {
      const v = await api.updateVersionNotes(id, notes);
      if (v) this.versions.update((list) => list.map((x) => (x.id === id ? v : x)));
      return true;
    } catch (e) {
      this.fail(e);
      return false;
    }
  }

  private resetDerived() {
    // A compare still running belongs to the old selection.
    this.compareRun++;
    this.comparing.set(false);
    this.comparison.set(null);
    this.refComparison.set(null);
    this.codec.set(null);
    this.streaming.set(null);
    this.section.set(null);
    this.sectionIndex.set(null);
    this.markIndex.set(null);
  }

  async selectVersion(id: number) {
    this.versionId.set(id);
    this.resetDerived();
    // Default partner: the previous bounce; for a reference, the latest bounce.
    const list = this.versions();
    const idx = list.findIndex((v) => v.id === id);
    const others = list.filter((v) => v.id !== id);
    const bounces = others.filter((v) => v.kind !== 'reference');
    const prev = list.slice(0, Math.max(idx, 0)).reverse().find((v) => v.kind !== 'reference');
    const partner = list[idx]?.kind === 'reference' ? bounces.at(-1) : prev ?? bounces[0];
    this.compareWith.set((partner ?? others[0])?.id ?? null);
    try {
      const report = await api.getReport(id);
      if (this.versionId() !== id) return;
      this.report.set(report);
      void this.locatePeaks(report);
      if (this.tab() === 'compare') await this.runCompare();
      if (this.tab() === 'streaming') await this.loadStreaming();
    } catch (e) {
      this.fail(e);
    }
  }

  async pickFiles(kind?: Kind) {
    if (!inTauri()) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({
      multiple: true,
      filters: [{ name: 'Audio', extensions: ['wav', 'wave', 'flac', 'aif', 'aiff', 'mp3', 'm4a'] }],
    });
    if (!picked) return;
    await this.addFiles(Array.isArray(picked) ? picked : [picked], kind);
  }

  /** Save the shareable report in the given format. */
  async exportReport(format: ExportFormat) {
    const v = this.version();
    if (!v || !inTauri() || this.exportState() === 'busy') return;
    const { save } = await import('@tauri-apps/plugin-dialog');
    const type = EXPORT_TYPES[format];
    const name = `${this.project()?.name ?? 'Song'} – ${v.label} report`.replace(/[\\/:*?"<>|]+/g, '-');
    const path = await save({ defaultPath: `${name}.${type.ext}`, filters: [{ name: type.name, extensions: [type.ext] }] });
    if (!path) return;
    this.exportState.set('busy');
    try {
      await api.exportReport(v.id, path, format);
      this.exportState.set('saved');
      setTimeout(() => this.exportState.set('idle'), 1600);
    } catch (e) {
      this.exportState.set('idle');
      this.fail(e);
    }
  }

  async addFiles(paths: string[], kind?: Kind) {
    const audio = paths.filter((p) => /\.(wav|wave|flac|aiff?|mp3|m4a)$/i.test(p));
    if (!audio.length) {
      this.error.set("MixLens can't read those files. Drop WAV, FLAC, AIFF, MP3 or M4A.");
      return;
    }
    let pid = this.projectId();
    if (pid == null) {
      const name = audio[0].split(/[\\/]/).pop()!.replace(/\.[^.]+$/, '').replace(/[_-](mix|master|v\d+|final).*$/i, '');
      try {
        const p = await api.createProject(name || 'New song');
        await this.refreshProjects();
        pid = p.id;
      } catch (e) {
        this.fail(e);
        return;
      }
      this.projectId.set(pid);
      this.versions.set([]);
      this.chat.set([]);
    }
    let lastId: number | null = null;
    for (const path of audio) {
      const name = path.split(/[\\/]/).pop()!;
      this.analyzing.update((a) => [...a, name]);
      try {
        const v = await api.addVersion(pid, path, kind);
        // The person may have opened another song while this one analyzed.
        if (this.projectId() === pid) this.versions.update((list) => [...list, v]);
        lastId = v.id;
      } catch (e) {
        this.fail(`${name}: ${errorText(e)}`);
      } finally {
        this.analyzing.update((a) => a.filter((x) => x !== name));
      }
    }
    try {
      await this.refreshProjects();
    } catch (e) {
      this.fail(e);
    }
    if (lastId == null || this.projectId() !== pid) return;
    if (kind === 'reference' && this.versionId() != null) {
      // Keep the current bounce in view and compare it with the new reference.
      this.compareWith.set(lastId);
      this.tab.set('compare');
      await this.runCompare();
    } else {
      await this.selectVersion(lastId);
    }
  }

  async updateVersion(label: string, kind: Kind) {
    const id = this.versionId();
    if (id == null) return;
    try {
      const v = await api.updateVersion(id, label, kind);
      this.versions.update((list) => list.map((x) => (x.id === id ? v : x)));
      await this.reloadReport(id);
      if (this.streaming()) await this.loadStreaming();
    } catch (e) {
      this.fail(e);
    }
  }

  async deleteVersion() {
    const id = this.versionId();
    if (id == null) return;
    try {
      await api.deleteVersion(id);
      const list = this.versions().filter((v) => v.id !== id);
      this.versions.set(list);
      this.versionId.set(null);
      this.report.set(null);
      await this.refreshProjects();
      const last = list.at(-1);
      if (last) await this.selectVersion(last.id);
    } catch (e) {
      this.fail(e);
    }
  }

  /** Open the Compare tab against a reference track. */
  async compareWithReference(id: number) {
    this.compareWith.set(id);
    this.tab.set('compare');
    await this.runCompare();
  }

  // ---------- measurements ----------

  async runCompare() {
    const a = this.compareWith();
    const b = this.versionId();
    if (a == null || b == null) return;
    // Only the latest run may show its result or end the busy state.
    const run = ++this.compareRun;
    this.comparing.set(true);
    this.comparison.set(null);
    this.refComparison.set(null);
    try {
      if (this.comparingReference()) {
        const [c, ref] = await Promise.all([api.compareReference(b, a), api.getReport(a)]);
        if (run !== this.compareRun) return;
        this.compareReport.set(ref);
        this.refComparison.set(c);
      } else {
        const [c, ref] = await Promise.all([api.compare(a, b), api.getReport(a)]);
        if (run !== this.compareRun) return;
        this.compareReport.set(ref);
        this.comparison.set(c);
      }
    } catch (e) {
      if (run === this.compareRun) this.fail(e);
    } finally {
      if (run === this.compareRun) this.comparing.set(false);
    }
  }

  async runCodec() {
    const id = this.versionId();
    if (id == null) return;
    this.codecBusy.set(true);
    try {
      const codec = await api.codecCheck(id);
      if (this.versionId() !== id) return;
      this.codec.set(codec);
      await this.loadStreaming();
    } catch (e) {
      this.fail(e);
    } finally {
      this.codecBusy.set(false);
    }
  }

  /** Per-platform breakdown; uses the latest encode check when one has run. */
  async loadStreaming() {
    const id = this.versionId();
    if (id == null) return;
    try {
      const streaming = await api.streamingReport(id);
      if (this.versionId() === id) this.streaming.set(streaming);
    } catch (e) {
      this.fail(e);
    }
  }

  async inspectSection(i: number) {
    const id = this.versionId();
    const s = this.report()?.report.sections[i];
    if (id == null || !s) return;
    if (this.sectionIndex() === i) {
      this.sectionIndex.set(null);
      this.section.set(null);
      return;
    }
    this.sectionIndex.set(i);
    try {
      const section = await api.section(id, s.start_s, s.end_s);
      if (this.versionId() === id && this.sectionIndex() === i) this.section.set(section);
    } catch (e) {
      this.fail(e);
    }
  }

  // ---------- assistant ----------

  async send(text: string) {
    const pid = this.projectId();
    if (pid == null || !text.trim() || this.chatBusy()) return;
    const pending: ChatMessage = { id: -Date.now(), role: 'user', content: text.trim(), created_at: '' };
    this.chat.update((c) => [...c, pending]);
    this.chatBusy.set(true);
    this.activity.set('Thinking');
    const focus = [this.versionId(), this.tab() === 'compare' ? this.compareWith() : null].filter((x): x is number => x != null);
    try {
      await api.chat(pid, text.trim(), focus);
      const chat = await api.getChat(pid);
      // Another song may be open now; its chat stays as it is.
      if (this.projectId() === pid) this.chat.set(chat);
    } catch (e) {
      if (this.projectId() === pid) {
        this.chat.update((c) => [...c, { id: -Date.now(), role: 'assistant', content: `**Couldn't get an answer.** ${errorText(e)}`, created_at: '' }]);
      }
    } finally {
      this.chatBusy.set(false);
      this.activity.set(null);
    }
  }

  async clearChat() {
    const pid = this.projectId();
    if (pid == null) return;
    try {
      await api.clearChat(pid);
      if (this.projectId() === pid) this.chat.set([]);
    } catch (e) {
      this.fail(e);
    }
  }
}
