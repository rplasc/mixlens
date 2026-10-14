import { Injectable, computed, signal } from '@angular/core';
import {
  api, ChatMessage, CodecResult, Comparison, errorText, GenrePreset, inTauri, Kind, MusicalKey, Project,
  ReferenceComparison, ReportEnvelope, SectionAnalysis, SettingsView, VersionSummary,
} from './api';

export type Tab = 'overview' | 'spectrum' | 'stereo' | 'compare';

const TOOL_LABELS: Record<string, string> = {
  get_report: 'Reading the report',
  compare_versions: 'Comparing versions',
  section_analysis: 'Measuring a section',
  codec_check: 'Encoding test files',
};

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
  readonly section = signal<SectionAnalysis | null>(null);
  readonly sectionIndex = signal<number | null>(null);

  readonly analyzing = signal<string[]>([]);
  readonly dragOver = signal(false);
  readonly error = signal<string | null>(null);

  readonly settings = signal<SettingsView | null>(null);
  readonly presets = signal<GenrePreset[]>([]);
  readonly showSettings = signal(false);

  readonly chat = signal<ChatMessage[]>([]);
  readonly chatBusy = signal(false);
  readonly activity = signal<string | null>(null);
  readonly chatOpen = signal(true);

  readonly project = computed(() => this.projects().find((p) => p.id === this.projectId()) ?? null);
  readonly version = computed(() => this.versions().find((v) => v.id === this.versionId()) ?? null);
  readonly otherVersions = computed(() => this.versions().filter((v) => v.id !== this.versionId()));
  readonly mixVersions = computed(() => this.versions().filter((v) => v.kind !== 'reference'));
  readonly references = computed(() => this.versions().filter((v) => v.kind === 'reference'));
  readonly genre = computed(() => this.presets().find((g) => g.id === this.project()?.genre) ?? null);
  /** The comparison in view is against a reference track rather than another bounce. */
  readonly comparingReference = computed(() => {
    const kind = (id: number | null) => this.versions().find((v) => v.id === id)?.kind;
    return kind(this.versionId()) === 'reference' || kind(this.compareWith()) === 'reference';
  });

  async init() {
    try {
      this.settings.set(await api.getSettings());
      this.presets.set(await api.listGenrePresets());
      const projects = await api.listProjects();
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

  async renameProject(name: string) {
    const id = this.projectId();
    if (id == null || !name.trim()) return;
    await api.renameProject(id, name);
    await this.refreshProjects();
  }

  async deleteProject() {
    const id = this.projectId();
    if (id == null) return;
    await api.deleteProject(id);
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
      this.versions.set(versions);
      this.chat.set(chat);
      const last = versions.filter((v) => v.kind !== 'reference').at(-1) ?? versions.at(-1);
      if (last) await this.selectVersion(last.id);
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
      this.versions.set(versions);
      const id = this.versionId();
      if (id != null) this.report.set(await api.getReport(id));
    } catch (e) {
      this.fail(e);
    }
  }

  // ---------- versions ----------

  private resetDerived() {
    this.comparison.set(null);
    this.refComparison.set(null);
    this.codec.set(null);
    this.section.set(null);
    this.sectionIndex.set(null);
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
      this.report.set(await api.getReport(id));
      if (this.tab() === 'compare') await this.runCompare();
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

  async addFiles(paths: string[], kind?: Kind) {
    const audio = paths.filter((p) => /\.(wav|wave|flac|aiff?|mp3|m4a)$/i.test(p));
    if (!audio.length) {
      this.error.set('Drop WAV, FLAC, AIFF, MP3 or M4A files.');
      return;
    }
    let pid = this.projectId();
    if (pid == null) {
      const name = audio[0].split(/[\\/]/).pop()!.replace(/\.[^.]+$/, '').replace(/[_-](mix|master|v\d+|final).*$/i, '');
      const p = await api.createProject(name || 'New project');
      await this.refreshProjects();
      pid = p.id;
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
        this.versions.update((list) => [...list, v]);
        lastId = v.id;
      } catch (e) {
        this.fail(`${name}: ${errorText(e)}`);
      } finally {
        this.analyzing.update((a) => a.filter((x) => x !== name));
      }
    }
    await this.refreshProjects();
    if (lastId == null) return;
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
      this.report.set(await api.getReport(id));
    } catch (e) {
      this.fail(e);
    }
  }

  async deleteVersion() {
    const id = this.versionId();
    if (id == null) return;
    await api.deleteVersion(id);
    const list = this.versions().filter((v) => v.id !== id);
    this.versions.set(list);
    this.versionId.set(null);
    this.report.set(null);
    await this.refreshProjects();
    const last = list[list.length - 1];
    if (last) await this.selectVersion(last.id);
  }

  // ---------- measurements ----------

  async runCompare() {
    const a = this.compareWith();
    const b = this.versionId();
    if (a == null || b == null) return;
    this.comparing.set(true);
    this.comparison.set(null);
    this.refComparison.set(null);
    try {
      if (this.comparingReference()) {
        const [c, ref] = await Promise.all([api.compareReference(b, a), api.getReport(a)]);
        this.compareReport.set(ref);
        this.refComparison.set(c);
      } else {
        const [c, ref] = await Promise.all([api.compare(a, b), api.getReport(a)]);
        this.compareReport.set(ref);
        this.comparison.set(c);
      }
    } catch (e) {
      this.fail(e);
    } finally {
      this.comparing.set(false);
    }
  }

  async runCodec() {
    const id = this.versionId();
    if (id == null) return;
    this.codecBusy.set(true);
    try {
      this.codec.set(await api.codecCheck(id));
    } catch (e) {
      this.fail(e);
    } finally {
      this.codecBusy.set(false);
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
      this.section.set(await api.section(id, s.start_s, s.end_s));
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
      this.chat.set(await api.getChat(pid));
    } catch (e) {
      this.chat.update((c) => [...c, { id: -Date.now(), role: 'assistant', content: `**Couldn't get an answer.** ${errorText(e)}`, created_at: '' }]);
    } finally {
      this.chatBusy.set(false);
      this.activity.set(null);
    }
  }

  async clearChat() {
    const pid = this.projectId();
    if (pid == null) return;
    await api.clearChat(pid);
    this.chat.set([]);
  }
}
