import { Component, ElementRef, OnDestroy, computed, effect, inject, signal, untracked, viewChild } from '@angular/core';
import { Finding, PeakKind } from './api';
import { BandChart } from './band-chart';
import { num, prose, signed, time } from './fmt';
import { LoudnessBridge } from './loudness-bridge';
import { Sev } from './severity';
import { Reading, Readings } from './readings';
import { findingsText } from './share';
import { Store } from './store';

const READING_CODES: Record<string, string[]> = {
  Integrated: ['loudness', 'loudness_low', 'loudness_high'],
  'True peak': ['tp_ceiling', 'mix_headroom', 'clipping', 'over_full_scale'],
  PLR: ['mix_overcompressed'],
  'Mono fold': ['mono'],
  Correlation: ['low_end_width'],
};

/** Findings whose problem can be shown on the loudness timeline, and the marker kind that locates it. */
const MARK_KIND: Record<string, PeakKind> = {
  over_full_scale: 'over_full_scale',
  clipping: 'clipping',
  tp_ceiling: 'true_peak',
  mix_headroom: 'true_peak',
};

/** Pause in typing before notes save. */
const NOTES_DELAY_MS = 800;

@Component({
  selector: 'ml-overview',
  imports: [LoudnessBridge, Readings, BandChart, Sev],
  template: `
    @if (r(); as r) {
      <div class="summary">
        @if (verdict(); as v) {
          <header class="verdict" [attr.data-sev]="v.sev">
            <h2><ml-sev [sev]="v.sev" [announce]="false" />{{ v.headline }}</h2>
            @if (v.sub) { <p class="muted">{{ v.sub }}</p> }
          </header>
        }
        @if (kindTip(); as tip) {
          <div class="tip" role="note">
            <p>{{ tip }}</p>
            @if (isMaster()) { <button class="link" (click)="store.showSettings.set(true)">Change platform</button> }
            <button class="ghost" (click)="store.dismissKindTip()">Got it</button>
          </div>
        }
        <div class="measures">
          <section class="group delivery">
            <h3>Delivery</h3>
            <ml-readings [items]="delivery()" size="l" />
          </section>
          <section class="group character">
            <h3>Dynamics and stereo</h3>
            <ml-readings [items]="character()" size="s" />
          </section>
        </div>
      </div>

      <section class="bridge" #bridge>
        <div class="bridge-head">
          <h2>Loudness over time</h2>
          <p class="muted">Short-term loudness in 3 s windows. Click a section to measure it on its own{{ marks().length ? ', or a peak marker for its exact time' : '' }}.</p>
        </div>
        <ml-loudness-bridge
          [values]="r.loudness.short_term" [hop]="r.loudness.short_term_hop_s" [sections]="r.sections"
          [integrated]="r.loudness.integrated_lufs" [target]="isMaster() ? target() : null"
          [duration]="r.file.duration_s" [clickable]="true" [selected]="store.sectionIndex()"
          (sectionClick)="store.inspectSection($event)"
          [marks]="marks()" [selectedMark]="store.markIndex()" (markClick)="toggleMark($event)" />
        @if (store.section(); as s) {
          <section class="inspect">
            <div class="inspect-head">
              <h2>{{ time(s.loudness.start_s) }} to {{ time(s.loudness.end_s) }}</h2>
              <span><strong>{{ num(s.loudness.lufs) }} LUFS</strong>
                <span class="muted">, {{ signed(s.loudness.vs_integrated_lu) }} LU vs whole song, tilt {{ num(s.bands.tilt_db_per_oct) }} dB/oct</span></span>
              <button class="ghost" (click)="store.inspectSection(store.sectionIndex()!)">Close</button>
            </div>
            <ml-band-chart [a]="s.bands.third_octave" [height]="150" />
          </section>
        }
      </section>

      <div class="cols">
        <section>
          <div class="find-head">
            <h2>Findings</h2>
            @if (!isReference()) {
              <button class="ghost small" (click)="copyFindings()" aria-live="polite">{{ copied() ? 'Copied' : 'Copy as text' }}</button>
            }
          </div>
          <ul class="findings">
            @for (f of issues(); track f.code + f.detail) {
              <li>
                <ml-sev [sev]="f.severity" />
                <div>
                  <strong>{{ prose(f.title) }}</strong>
                  <p>{{ prose(f.detail) }}</p>
                  @if (f.code === 'genre_tone' && f.severity !== 'ok' && store.references()[0]; as ref) {
                    <button class="link" (click)="store.compareWithReference(ref.id)">Compare with {{ ref.label }}</button>
                  }
                  @let mark = loudestMark(f);
                  @if (mark !== null) {
                    <button class="link" (click)="showMark(mark)">Show on timeline</button>
                  }
                </div>
              </li>
            }
          </ul>
          @if (passed().length) {
            <p class="passed"><span class="sev-ok">Passed:</span> {{ passed().join(' · ') }}</p>
          }
        </section>

        <div class="aside">
        <section class="notes">
          <div class="notes-head">
            <h2><label for="version-notes">Notes</label></h2>
            <span class="faint" aria-live="polite">{{ notesSaved() ? 'Saved' : '' }}</span>
          </div>
          <textarea #notesBox id="version-notes" rows="4" (input)="notesInput(notesBox.value)" (blur)="flushNotes()"
            placeholder="What changed in this bounce, or what to fix next. Only you and the assistant see these."></textarea>
        </section>

        <section>
          <h2>File</h2>
          <table class="data">
            <tbody>
              <tr><td class="muted">First sound</td><td>{{ time(r.edges.first_sound_s) }}</td></tr>
              <tr><td class="muted" title="The last moment above −60 dBFS">Last sound</td><td>{{ time(r.edges.last_sound_s) }}</td></tr>
              <tr><td class="muted">File ends</td><td>{{ time(r.file.duration_s) }}, {{ ending() }}</td></tr>
              <tr><td class="muted">Fade-out</td><td>{{ r.edges.fade_out_detected ? 'Yes' : 'No' }}</td></tr>
              <tr><td class="muted">Sample peak</td><td>L {{ num(r.loudness.sample_peak_l_dbfs, 2) }}, R {{ num(r.loudness.sample_peak_r_dbfs, 2) }} dBFS</td></tr>
              <tr><td class="muted">Crest factor</td><td>{{ num(r.loudness.crest_db) }} dB</td></tr>
              @if (r.dither) {
                <tr><td class="muted">Dither</td><td>{{ r.dither.detected ? 'Present' : r.dither.samples_checked ? 'Not detected' : 'Not checked' }}
                  <span class="faint">({{ r.dither.bits }}-bit)</span></td></tr>
              }
            </tbody>
          </table>
        </section>
        </div>
      </div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--space-xl); }
    section h2 { margin-bottom: var(--space-xs); }
    .summary { display: flex; flex-direction: column; gap: var(--space-m); }
    .measures { display: flex; flex-wrap: wrap; gap: var(--space-s) var(--space-l); }
    .group h3 { margin-bottom: var(--space-2xs); }
    /* Pull values left so they line up with the verdict; the severity rule sits in the gutter. */
    .group ml-readings { display: block; margin-left: -14px; }
    .delivery { flex: 0 0 260px; }
    .character { flex: 1 1 380px; max-width: 540px; }
    .verdict h2 { font-size: var(--text-title); display: flex; align-items: center; gap: var(--space-xs); }
    .verdict p { margin: 2px 0 0 18px; max-width: 80ch; }
    .bridge-head { display: flex; align-items: baseline; flex-wrap: wrap; column-gap: var(--space-s); margin-bottom: var(--space-xs); }
    .bridge-head p { font-size: var(--text-caption); }
    .inspect { margin-top: var(--space-s); border: 1px solid var(--line); border-radius: var(--radius-m); padding: var(--space-s) var(--space-m); background: var(--panel); }
    .inspect-head { display: flex; align-items: baseline; gap: var(--space-s); margin-bottom: var(--space-xs); }
    .inspect-head button { margin-left: auto; }
    .cols { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--space-xl); }
    .findings { list-style: none; margin: 0; padding: 0; }
    .findings li { display: grid; grid-template-columns: 14px 1fr; gap: var(--space-xs); padding: var(--space-s) 0; border-bottom: 1px solid var(--line-soft); }
    .findings li:first-child { padding-top: var(--space-2xs); }
    .findings p { color: var(--muted); max-width: 70ch; }
    .findings ml-sev { margin-top: 5px; }
    .passed { margin-top: var(--space-s); font-size: var(--text-body); color: var(--muted); }
    .findings .link { margin-top: var(--space-2xs); margin-right: var(--space-s); }
    .find-head, .notes-head { display: flex; align-items: baseline; gap: var(--space-s); margin-bottom: var(--space-xs); }
    .find-head h2, .notes-head h2 { margin-bottom: 0; }
    .find-head .small { font-size: var(--text-caption); padding: 2px 8px; }
    .notes-head span { font-size: var(--text-caption); }
    .aside { display: flex; flex-direction: column; gap: var(--space-l); min-width: 0; }
    .notes textarea { display: block; width: 100%; resize: vertical; min-height: 5.5em; }
    .tip { display: flex; align-items: baseline; flex-wrap: wrap; gap: var(--space-2xs) var(--space-s); max-width: 80ch;
      padding: var(--space-xs) var(--space-s); border: 1px solid var(--line); border-radius: var(--radius-s); font-size: var(--text-caption); color: var(--muted); }
    .tip p { flex: 1 1 40ch; }
    .tip .ghost { font-size: var(--text-caption); padding: 2px 8px; }
    /* Findings and file details sit side by side only when the main column is wide enough. */
    @container (min-width: 820px) { .cols { grid-template-columns: minmax(0, 1.6fr) minmax(260px, 1fr); } }
  `,
})
export class Overview implements OnDestroy {
  readonly store = inject(Store);
  readonly num = num;
  readonly signed = signed;
  readonly time = time;
  readonly prose = prose;
  readonly r = computed(() => this.store.report()?.report ?? null);

  /** How the file ends, in the tail finding's terms: silence, a faint tail, or sound still playing (above -60 dBFS). */
  readonly ending = computed(() => {
    const e = this.r()?.edges;
    if (!e) return '';
    if (e.ends_on_silence) return `after ${num(e.trailing_silence_s)} s of silence`;
    const db = num(e.tail_last50ms_dbfs, 0);
    return e.tail_last50ms_dbfs > -60 ? `cut off while still playing (${db} dBFS)` : `with a faint tail still at ${db} dBFS`;
  });
  /** Findings that need reading; passed checks collapse into one line. */
  readonly issues = computed(() => this.r()?.findings.filter((f) => f.severity !== 'ok') ?? []);
  readonly passed = computed(() => this.r()?.findings.filter((f) => f.severity === 'ok').map((f) => prose(f.title)) ?? []);
  readonly isMaster = computed(() => this.store.report()?.kind === 'master');
  readonly isReference = computed(() => this.store.report()?.kind === 'reference');
  readonly marks = computed(() => this.r()?.peak_marks ?? []);

  private readonly bridge = viewChild<ElementRef<HTMLElement>>('bridge');
  private readonly notesBox = viewChild<ElementRef<HTMLTextAreaElement>>('notesBox');
  /** Typed but not yet saved, with the version it was typed for. */
  private notesDraft: { id: number; text: string } | null = null;
  private notesTimer: ReturnType<typeof setTimeout> | undefined;
  readonly notesSaved = signal(false);
  readonly copied = signal(false);

  constructor() {
    // Fill the box when another version opens. Not tied to the saved text, so a save that
    // lands mid-typing doesn't overwrite what's in the box.
    effect(() => {
      const id = this.store.versionId();
      const box = this.notesBox()?.nativeElement;
      untracked(() => {
        void this.flushNotes();
        this.notesSaved.set(false);
        if (box) box.value = this.store.versions().find((v) => v.id === id)?.notes ?? '';
      });
    });
  }

  ngOnDestroy() {
    void this.flushNotes();
  }

  notesInput(text: string) {
    const id = this.store.versionId();
    if (id === null) return;
    this.notesDraft = { id, text };
    this.notesSaved.set(false);
    clearTimeout(this.notesTimer);
    this.notesTimer = setTimeout(() => void this.flushNotes(), NOTES_DELAY_MS);
  }

  async flushNotes() {
    clearTimeout(this.notesTimer);
    const d = this.notesDraft;
    this.notesDraft = null;
    if (!d) return;
    // Skip only a draft known to match; a version missing from the list (another song open) is saved.
    const saved = this.store.versions().find((v) => v.id === d.id)?.notes;
    if (saved !== undefined && d.text.trim() === saved) return;
    const ok = await this.store.saveNotes(d.id, d.text);
    if (ok && this.store.versionId() === d.id && !this.notesDraft) this.notesSaved.set(true);
  }

  /** Index of the loudest marker that locates this finding's problem, if any. */
  loudestMark(f: Finding): number | null {
    const kind = MARK_KIND[f.code];
    if (!kind || f.severity === 'ok') return null;
    const marks = this.marks();
    let best: number | null = null;
    marks.forEach((m, i) => {
      if (m.kind === kind && (best === null || m.peak_db > marks[best].peak_db)) best = i;
    });
    return best;
  }

  toggleMark(i: number) {
    this.store.markIndex.set(this.store.markIndex() === i ? null : i);
  }

  showMark(i: number) {
    this.store.markIndex.set(i);
    this.bridge()?.nativeElement.scrollIntoView({ block: 'nearest' });
  }

  async copyFindings() {
    const r = this.r();
    const v = this.store.version();
    if (!r || !v) return;
    const l = r.loudness;
    const text = findingsText({
      song: this.store.project()?.name ?? 'Untitled song',
      version: v.label,
      kind: v.kind,
      verdict: this.verdict(),
      readings: [`${num(l.integrated_lufs)} LUFS integrated`, `${num(l.true_peak_dbtp)} dBTP true peak`, `PLR ${num(l.plr_db)} dB`],
      findings: r.findings,
    });
    try {
      await navigator.clipboard.writeText(text);
      this.copied.set(true);
      setTimeout(() => this.copied.set(false), 1600);
    } catch {
      this.store.fail("Couldn't copy to the clipboard.");
    }
  }
  readonly target = this.store.target;
  readonly platformName = computed(() => this.store.mainPlatform()?.name ?? null);
  /** "Spotify target −14", or "Target −16" for a custom target. */
  readonly targetNote = computed(() => {
    const name = this.platformName();
    return `${name ? name + ' target' : 'Target'} ${num(this.target(), 0)}`;
  });

  /** Shown until dismissed: which rules ran, since the file name picked the type. */
  readonly kindTip = computed(() => {
    const kind = this.store.report()?.kind;
    if (this.store.kindTipSeen() || !kind || kind === 'reference') return null;
    if (kind === 'mix') {
      return 'MixLens checked this as a mix, so it looked at headroom and bus compression before mastering. If this is a finished master, switch to Master above to check it against your streaming target.';
    }
    const where = this.platformName() ? `${this.platformName()}'s` : 'your';
    return `MixLens checked this as a master against ${where} ${num(this.target(), 0)} LUFS target and true-peak ceiling.`;
  });

  /**
   * One-line answer to "is this ready, and what do I fix first?", from the findings.
   * The exported report has a Rust copy (`export::verdict` in mixlens-core); keep the two in step.
   */
  readonly verdict = computed(() => {
    const r = this.r();
    const kind = this.store.report()?.kind;
    if (!r || kind === 'reference') return null;
    const errors = r.findings.filter((f) => f.severity === 'error');
    const warns = r.findings.filter((f) => f.severity === 'warn');
    const ok = (code: string) => r.findings.some((f) => f.code === code && f.severity === 'ok');
    const goal = kind === 'master' ? 'before release' : 'before mastering';
    const where = this.platformName() ? `for ${this.platformName()}` : 'for your target';

    const top = errors[0] ?? warns[0];
    // What already passed, so a list of problems doesn't read as "nothing works".
    const already = top ? 'already ' : '';
    let passed = '';
    if (kind === 'master') {
      if (ok('loudness') && ok('tp_ceiling')) passed = `Loudness and true peak are ${already}on target ${where}.`;
      else if (ok('tp_ceiling')) passed = `True peak is ${already}under the ceiling ${where}.`;
      else if (ok('loudness')) passed = `Loudness is ${already}on target ${where}.`;
    } else if (ok('mix_headroom') && !r.findings.some((f) => f.code === 'mix_overcompressed')) {
      passed = 'Headroom and dynamics leave the master room to work.';
    }

    if (!top) return { sev: 'ok' as const, headline: 'No problems found', sub: passed };
    const n = errors.length || warns.length;
    const headline = errors.length
      ? `${n === 1 ? 'One problem' : `${n} problems`} to fix ${goal}`
      : `${n === 1 ? 'One thing' : `${n} things`} to check ${goal}`;
    return { sev: errors.length ? ('error' as const) : ('warn' as const), headline, sub: `Start with: ${prose(top.title)}. ${passed}`.trim() };
  });

  private sevFor(label: string, findings: Finding[]) {
    const codes = READING_CODES[label] ?? [];
    const hit = findings.filter((f) => codes.includes(f.code));
    if (!hit.length) return null;
    const order = ['ok', 'info', 'warn', 'error'];
    return hit.map((f) => f.severity).sort((a, b) => order.indexOf(b) - order.indexOf(a))[0];
  }

  readonly delivery = computed(() => this.readings().slice(0, 2));
  readonly character = computed(() => this.readings().slice(2));

  readonly readings = computed<Reading[]>(() => {
    const r = this.r();
    if (!r) return [];
    const l = r.loudness;
    const s = r.stereo;
    const f = r.findings;
    return [
      { label: 'Integrated', value: num(l.integrated_lufs), unit: 'LUFS', note: this.isMaster() ? this.targetNote() : undefined, severity: this.sevFor('Integrated', f) },
      { label: 'True peak', value: num(l.true_peak_dbtp), unit: 'dBTP', severity: this.sevFor('True peak', f) },
      { label: 'Loudness range', value: num(l.lra_lu), unit: 'LU' },
      { label: 'PLR', value: num(l.plr_db), unit: 'dB', note: 'peak to loudness', severity: this.sevFor('PLR', f) },
      { label: 'Mono fold', value: signed(s.mono_loss_lu, 2), unit: 'LU', severity: this.sevFor('Mono fold', f) },
      { label: 'Correlation', value: num(s.correlation, 2), note: `${num(s.low_correlation, 2)} below 120 Hz`, severity: this.sevFor('Correlation', f) },
    ];
  });
}
