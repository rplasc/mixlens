import { Component, DestroyRef, computed, effect, inject, input, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AbPlayer, Contour } from './ab-player';
import { BandChart } from './band-chart';
import { hostWidth } from './dom';
import { clock, hz, num, signed } from './fmt';
import { LoudnessBridge } from './loudness-bridge';
import { Player, PlayPair } from './player';
import { Store } from './store';
import { Trend } from './trend';

/** Per-window null-test strip aligned with the loudness timeline's x axis. */
@Component({
  selector: 'ml-diff-strip',
  template: `
    <svg [attr.width]="w()" height="34" role="img" aria-label="Where the two versions differ">
      @for (c of cells(); track $index) {
        <rect [attr.x]="c.x" y="2" [attr.width]="c.w" height="22" [style.fill]="c.fill"><title>{{ c.title }}</title></rect>
      }
      <text x="0" y="17" class="axis">Diff</text>
    </svg>
  `,
  styles: `:host { display: block; width: 100%; } svg { display: block; } .axis { fill: var(--faint); font-size: var(--text-caption); }`,
})
export class DiffStrip {
  readonly windows = input.required<number[]>();
  readonly windowS = input(2);
  readonly duration = input(1);
  /** Nearly everything differs: scale to this pair's own range, dimmer, so the strip shows where it changed most instead of one solid bar. */
  readonly relative = input(false);
  readonly left = 34;
  readonly w = hostWidth(900, 300);
  readonly cells = computed(() => {
    const span = Math.max(this.duration(), this.windows().length * this.windowS());
    const px = (t: number) => this.left + (t / span) * (this.w() - this.left);
    const heard = this.windows().filter((d) => d > -60);
    const lo = this.relative() && heard.length ? Math.min(...heard) : -60;
    const hi = this.relative() && heard.length ? Math.max(...heard) : 0;
    const [base, reach] = this.relative() ? [20, 30] : [20, 75];
    return this.windows().map((d, i) => {
      const t0 = i * this.windowS();
      // One neutral ramp, brighter as the difference grows: a change between bounces isn't a problem, so no severity colours.
      const mix = Math.round(base + Math.max(0, Math.min(1, (d - lo) / (hi - lo || 1))) * reach);
      const fill = d <= -60 ? 'var(--line-soft)' : `color-mix(in oklab, var(--ink) ${mix}%, var(--line-soft))`;
      const label = d <= -100 ? 'identical (silence)' : d <= -60 ? `identical (${num(d, 0)} dB)` : `${num(d)} dB difference`;
      return { x: px(t0), w: Math.max(1, px(t0 + this.windowS()) - px(t0) - 0.5), fill, title: `${clock(t0)}: ${label}` };
    });
  });
}

@Component({
  selector: 'ml-compare',
  imports: [FormsModule, AbPlayer, LoudnessBridge, DiffStrip, BandChart, Trend],
  template: `
    @if (!store.otherVersions().length) {
      <div class="first">
        <h2>Nothing to compare yet</h2>
        <p class="muted">Add another bounce of this song to see what changed, second by second, or add a finished reference track to compare level and tonal balance against.</p>
        <div class="first-actions">
          <button class="primary" (click)="store.pickFiles()">Add files</button>
          <button (click)="store.pickFiles('reference')">Add reference</button>
        </div>
      </div>
    } @else {
      @if (trendShown()) {
        <section>
          <div class="head">
            <h2>All versions</h2>
            <p class="muted">Select one to compare it with {{ store.version()?.label }}.</p>
          </div>
          <ml-trend />
        </section>
      }
      <div class="picker" [class.split]="trendShown()">
        <label>
          <span class="muted">Compare</span>
          <strong class="a">{{ store.version()?.label }}</strong>
          <span class="muted">against</span>
          <select [ngModel]="store.compareWith()" (ngModelChange)="store.compareWith.set($event)">
            @if (otherBounces().length) {
              <optgroup label="Versions">@for (v of otherBounces(); track v.id) { <option [ngValue]="v.id">{{ v.label }}</option> }</optgroup>
            }
            @if (otherRefs().length) {
              <optgroup label="References">@for (v of otherRefs(); track v.id) { <option [ngValue]="v.id">{{ v.label }}</option> }</optgroup>
            }
          </select>
        </label>
        <button class="primary" (click)="store.runCompare()" [disabled]="store.comparing() || store.compareWith() === null">
          {{ store.comparing() ? 'Comparing…' : 'Compare' }}
        </button>
        <p class="muted status" role="status">
          @if (store.comparing()) { {{ store.comparingReference() ? 'Comparing with the reference…' : 'Aligning the files and running the null test…' }} }
        </p>
      </div>
    }

    @if (store.comparing()) {
      <!-- Progress is announced beside the Compare button. -->
    } @else if (rc(); as rc) {
      <p class="verdict">{{ refVerdict() }}</p>
      <ml-ab-player [currentLabel]="label(rc.version_id)" [partnerLabel]="label(rc.reference_id)" [contour]="refContour()" />

      <div class="cols">
        <section>
          <h2>Measurements</h2>
          <table class="data">
            <thead><tr><th>Metric</th><th class="n b">Reference</th><th class="n a">{{ label(rc.version_id) }}</th><th class="n">Difference</th></tr></thead>
            <tbody>
              @for (m of rc.metric_deltas; track m.name) {
                <tr>
                  <td>{{ metricLabel(m.name) }}</td>
                  <td class="n">{{ num(m.a, digits(m.name)) }}</td>
                  <td class="n">{{ num(m.b, digits(m.name)) }}</td>
                  <td class="n">{{ signed(m.delta, digits(m.name)) }} <span class="faint">{{ m.unit }}</span></td>
                </tr>
              }
            </tbody>
          </table>
        </section>
        <section>
          <h2>Energy by octave</h2>
          <table class="data">
            <thead><tr><th>Band</th><th class="n b">Reference</th><th class="n a">{{ label(rc.version_id) }}</th><th class="n">Difference</th></tr></thead>
            <tbody>
              @for (o of rc.octave; track o.lo) {
                <tr>
                  <td>{{ hz(o.lo) }}–{{ hz(o.hi) }} Hz</td>
                  <td class="n">{{ num(o.reference_db) }} dB</td>
                  <td class="n">{{ num(o.version_db) }} dB</td>
                  <td class="n" [style.color]="cellColor(o.delta_db)">{{ signed(o.delta_db) }} dB</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      </div>

      <section>
        <h2>Tonal difference from {{ label(rc.reference_id) }}</h2>
        <p class="muted small">{{ label(rc.version_id) }} minus the reference by third octave, level-matched over 100 Hz–5 kHz. Up means more energy than the reference.</p>
        <ml-band-chart [a]="rc.third_octave_delta" [height]="200" />
      </section>
    } @else if (c(); as c) {
      <p class="verdict">{{ verdict() }}</p>

      <section>
        <!-- The transport sits on the timeline it drives; its name line is the chart's colour key. -->
        <ml-ab-player class="transport" [currentLabel]="store.version()?.label ?? ''" [partnerLabel]="partner()?.label ?? ''" />
        @if (partnerReport(); as pr) {
          <ml-loudness-bridge [values]="current()!.loudness.short_term" [compareValues]="pr.loudness.short_term"
            [hop]="current()!.loudness.short_term_hop_s" [sections]="current()!.sections" [duration]="span()"
            [playhead]="playhead()" [focus]="chartFocus()" [seekable]="true" (seek)="player.seek($event)" />
        }
        <ml-diff-strip [windows]="c.windows" [windowS]="c.window_s" [duration]="span()" [relative]="mostlyChanged()" />
        <p class="faint small">
          @if (mostlyChanged()) { Almost every stretch changed, so brightness here only shows where it changed most. }
          @else { Dark: identical. The brighter a stretch, the more the audio changed; faint differences are often time-varying plugins. }
        </p>
      </section>

      <div class="cols">
        <section>
          <h2>Sections</h2>
          <table class="data">
            <thead><tr><th>Range</th><th>Type</th><th class="n b">Before</th><th class="n a">Now</th><th class="n">Change</th></tr></thead>
            <tbody>
              @for (s of c.section_deltas; track s.start_s) {
                <tr>
                  <td>{{ clock(s.start_s) }}–{{ clock(s.end_s) }}</td>
                  <td class="muted">{{ s.kind }}</td>
                  <td class="n">{{ num(s.lufs_a) }}</td>
                  <td class="n">{{ num(s.lufs_b) }}</td>
                  <td class="n" [class.sev-warn]="abs(s.delta_lu) >= 1">{{ signed(s.delta_lu) }} LU</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
        <section>
          <h2>Measurements</h2>
          <table class="data">
            <thead><tr><th>Metric</th><th class="n b">Before</th><th class="n a">Now</th><th class="n">Change</th></tr></thead>
            <tbody>
              @for (m of c.metric_deltas; track m.name) {
                <tr>
                  <td>{{ metricLabel(m.name) }}</td>
                  <td class="n">{{ num(m.a, digits(m.name)) }}</td>
                  <td class="n">{{ num(m.b, digits(m.name)) }}</td>
                  <td class="n">{{ signed(m.delta, digits(m.name)) }} <span class="faint">{{ m.unit }}</span></td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      </div>

      <section>
        <h2>Tonal balance</h2>
        <p class="muted small">Deviation from each version's own tilt, by third octave.</p>
        <ml-band-chart [a]="currentBands()" [b]="partnerBands()" [height]="200" />
      </section>

      @if (c.changed_regions.length) {
        <section>
          <h2>Changed regions</h2>
          <p class="muted small">Change in energy per octave band. Each column is named by its band's lower edge, in Hz.</p>
          <table class="data regions">
            <thead>
              <tr><th>Range</th><th class="n">Loudness</th>
                @for (b of octaveLabels(); track b) { <th class="n oct">{{ b }}</th> }
              </tr>
            </thead>
            <tbody>
              @for (g of c.changed_regions; track g.start_s) {
                <tr>
                  <td>{{ clock(g.start_s) }}–{{ clock(g.end_s) }}</td>
                  <td class="n">{{ signed(g.lufs_b - g.lufs_a) }} LU</td>
                  @for (d of g.octave_delta_db; track $index) {
                    <td class="n oct" [style.color]="cellColor(d)">{{ signed(d) }}</td>
                  }
                </tr>
              }
            </tbody>
          </table>
        </section>
      }
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--space-l); }
    .head { display: flex; align-items: baseline; flex-wrap: wrap; column-gap: var(--space-s); margin-bottom: var(--space-xs); }
    .head h2 { margin: 0; }
    .head p { font-size: var(--text-caption); }
    .picker { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-s); }
    /* The version history above, the pairwise comparison below. */
    .picker.split { border-top: 1px solid var(--line-soft); margin-top: var(--space-s); padding-top: var(--space-l); }
    .picker .status { font-size: var(--text-caption); }
    .first { max-width: 60ch; display: flex; flex-direction: column; gap: var(--space-xs); }
    .first-actions { display: flex; gap: var(--space-xs); margin-top: var(--space-xs); }
    .picker label { display: flex; align-items: center; gap: var(--space-xs); }
    .verdict { font-size: var(--text-subhead); max-width: 80ch; }
    .transport { margin-bottom: var(--space-xs); }
    .small { font-size: var(--text-caption); }
    h2 { margin-bottom: var(--space-xs); }
    .cols { display: grid; grid-template-columns: 1fr 1fr; gap: var(--space-xl); }
    .regions .oct { font-size: var(--text-caption); }
    @container (max-width: 819px) { .cols { grid-template-columns: 1fr; } }
  `,
})
export class Compare {
  readonly store = inject(Store);
  readonly player = inject(Player);
  readonly num = num;
  readonly signed = signed;
  readonly clock = clock;
  readonly abs = Math.abs;
  readonly c = computed(() => this.store.comparison());
  /** Each band's lower edge; the backend sends bands as "lo-hi". */
  readonly octaveLabels = computed(() => (this.c()?.octave_bands ?? []).map((b) => hz(Number(b.split('-')[0]))));
  readonly rc = computed(() => this.store.refComparison());
  readonly hz = hz;
  readonly otherBounces = computed(() => this.store.otherVersions().filter((v) => v.kind !== 'reference'));
  readonly otherRefs = computed(() => this.store.otherVersions().filter((v) => v.kind === 'reference'));
  /** With two bounces, the pairwise view below already shows the whole story. */
  readonly trendShown = computed(() => this.store.bounces().length >= 3);

  label(id: number) {
    return this.store.versions().find((v) => v.id === id)?.label ?? '';
  }

  readonly refVerdict = computed(() => {
    const rc = this.rc();
    if (!rc) return '';
    const d = rc.level_match_db;
    const level = Math.abs(d) < 0.5
      ? 'Both play at about the same loudness, so you can A/B them directly.'
      : `The reference is ${num(Math.abs(d))} dB ${d > 0 ? 'louder' : 'quieter'}; the player turns ${d > 0 ? 'B (the reference)' : 'A (this version)'} down to match.`;
    const big = rc.octave.filter((o) => Math.abs(o.delta_db) >= 1.5).sort((a, b) => Math.abs(b.delta_db) - Math.abs(a.delta_db)).slice(0, 3);
    const tone = big.length
      ? ` Largest tonal differences: ${big.map((o) => `${hz(o.lo)}–${hz(o.hi)} Hz ${signed(o.delta_db)} dB`).join(', ')}.`
      : ' Every octave is within 1.5 dB of the reference.';
    const keys = rc.version_key && rc.reference_key && rc.version_key !== rc.reference_key
      ? ` The reference is in ${rc.reference_key} and this song in ${rc.version_key}, so their bass notes sit at different pitches: compare the weight of the low end, not individual peaks.`
      : '';
    const genres = rc.version_genre && rc.reference_genre && rc.version_genre !== rc.reference_genre
      ? ` It's also a different genre (${rc.reference_genre} vs ${rc.version_genre}).`
      : '';
    return level + tone + keys + genres;
  });
  readonly current = computed(() => this.store.report()?.report ?? null);
  /** The version compared against (another bounce here; the reference view has its own data). */
  readonly partner = computed(() => this.store.compareReport());
  readonly partnerReport = computed(() => this.partner()?.report ?? null);
  /** Shared time axis for the timeline and diff strip: the longer of the two versions. */
  readonly span = computed(() => Math.max(this.current()?.file.duration_s ?? 0, this.partnerReport()?.file.duration_s ?? 0, 1));
  // The backend compares a = partner with b = the open version.
  /** What the A/B player plays: the pair this view measured, with its level and time offsets. */
  readonly playPair = computed<PlayPair | null>(() => {
    const cur = this.store.report();
    const part = this.partner();
    if (!cur || !part) return null;
    const dur = (id: number) => (id === cur.id ? cur : part).report.file.duration_s;
    const rc = this.rc();
    if (rc) {
      // The version is always gold and the reference cyan, as in the tables, whichever one is open.
      const durations = { current: dur(rc.version_id), partner: dur(rc.reference_id) };
      return { mode: 'reference', current: rc.version_id, partner: rc.reference_id, lagS: 0, loudnessDiff: rc.level_match_db ?? NaN, durations };
    }
    const c = this.c();
    if (!c) return null;
    // a = the partner, b = the open version.
    const m = c.metric_deltas.find((x) => x.name === 'integrated');
    const loudnessDiff = m && m.a != null && m.b != null ? m.a - m.b : NaN;
    const durations = { current: cur.report.file.duration_s, partner: part.report.file.duration_s };
    return { mode: 'version', current: cur.id, partner: part.id, lagS: c.lag_ms / 1000, loudnessDiff, durations };
  });
  /** Reference mode has no timeline, so the player draws each side's loudness above its position bar. */
  readonly refContour = computed<Contour | null>(() => {
    const cur = this.store.report();
    const part = this.partner();
    const rc = this.rc();
    if (!cur || !part || !rc) return null;
    const loud = (id: number) => (id === cur.id ? cur : part).report.loudness;
    const v = loud(rc.version_id);
    return { current: v.short_term, partner: loud(rc.reference_id).short_term, hop: v.short_term_hop_s };
  });
  /** The player's position on the timeline, once it has been used, in the audible side's colour. */
  readonly playhead = computed(() =>
    this.player.touched() ? { t: this.player.position(), side: this.chartSide(), live: this.player.playing() } : null);
  /** The series being heard, dimming the other; none until the player is used. */
  readonly chartFocus = computed(() => (this.player.touched() ? this.chartSide() : null));
  private readonly chartSide = computed(() => (this.player.side() === 'current' ? 'a' : 'b') as 'a' | 'b');

  constructor() {
    effect(() => {
      const pair = this.playPair();
      // Only the pair drives this; the player's own state (playing, side) must not re-run it.
      untracked(() => this.player.setPair(pair));
    });
    inject(DestroyRef).onDestroy(() => this.player.pause());
  }

  readonly currentBands = computed(() => (this.c()?.third_octave_delta ?? []).map((d) => ({ fc: d.fc, level_db: 0, dev_db: d.dev_b })));
  readonly partnerBands = computed(() => (this.c()?.third_octave_delta ?? []).map((d) => ({ fc: d.fc, level_db: 0, dev_db: d.dev_a })));

  /** At least 80% of the song differs, the same test as the verdict's time-varying-plugins note. */
  readonly mostlyChanged = computed(() => {
    const w = this.c()?.windows ?? [];
    const total = w.filter((d) => d > -100).length;
    return total > 0 && w.filter((d) => d > -20).length / total > 0.8;
  });

  readonly verdict = computed(() => {
    const c = this.c();
    if (!c) return '';
    const changed = c.windows.filter((d) => d > -20).length;
    const total = c.windows.filter((d) => d > -100).length || 1;
    if (c.residual_rel_db < -55) return `These versions null to ${num(c.residual_rel_db, 0)} dB, so the audio is effectively identical${c.resampled ? ' after resampling to the same rate' : ''}.`;
    const pct = Math.round((changed / total) * 100);
    const moved = c.section_deltas.filter((s) => Math.abs(s.delta_lu) >= 0.5);
    const where = moved.length
      ? ` Loudness moved in ${moved.map((s) => `${clock(s.start_s)}–${clock(s.end_s)} (${signed(s.delta_lu)} LU)`).join(' and ')}; every other section is within 0.5 LU.`
      : ' Every section is within 0.5 LU of the other version.';
    const cause = pct > 80 ? ' When almost everything differs, time-varying plugins or re-rendered effects are a likely cause.' : '';
    return `${pct}% of the song doesn't null against the other version.${cause}${where}`;
  });

  metricLabel(name: string) {
    return ({
      integrated: 'Integrated', true_peak: 'True peak', lra: 'Loudness range', plr: 'PLR', crest: 'Crest factor',
      correlation: 'Correlation', low_correlation: 'Correlation < 120 Hz', mono_loss: 'Mono fold', duration: 'Length',
      trailing_silence: 'Silence at end', tilt: 'Tilt', sibilance_p99: '5–10 kHz peaks', side_mid: 'Side vs mid',
      sub30: 'Below 30 Hz',
    } as Record<string, string>)[name] ?? name;
  }
  digits(name: string) {
    return name.includes('correlation') ? 2 : 1;
  }
  cellColor(d: number) {
    if (!isFinite(d) || Math.abs(d) < 0.5) return 'var(--faint)';
    return d > 0 ? 'var(--a)' : 'var(--b)';
  }
}
