import { Component, computed, inject, input } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { BandChart } from './band-chart';
import { hostWidth } from './dom';
import { clock, hz, num, signed } from './fmt';
import { LoudnessBridge } from './loudness-bridge';
import { Store } from './store';

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
  readonly left = 34;
  readonly w = hostWidth(900, 300);
  readonly cells = computed(() => {
    const span = Math.max(this.duration(), this.windows().length * this.windowS());
    const px = (t: number) => this.left + (t / span) * (this.w() - this.left);
    return this.windows().map((d, i) => {
      const t0 = i * this.windowS();
      const mix = Math.round((0.25 + ((d + 60) / 40) * 0.35) * 100);
      const fill = d <= -60 ? 'var(--line-soft)' : d <= -20 ? `color-mix(in srgb, var(--b) ${mix}%, transparent)` : d <= -6 ? 'var(--warn)' : 'var(--err)';
      const label = d <= -100 ? 'identical (silence)' : d <= -60 ? `identical (${num(d, 0)} dB)` : `${num(d)} dB difference`;
      return { x: px(t0), w: Math.max(1, px(t0 + this.windowS()) - px(t0) - 0.5), fill, title: `${clock(t0)}: ${label}` };
    });
  });
}

@Component({
  selector: 'ml-compare',
  imports: [FormsModule, LoudnessBridge, DiffStrip, BandChart],
  template: `
    @if (store.otherVersions().length) {
    <div class="picker">
      <label>
        <span class="muted">Compare</span>
        <strong class="a">{{ store.version()?.label }}</strong>
        <span class="muted">against</span>
        <select [ngModel]="store.compareWith()" (ngModelChange)="store.compareWith.set(+$event)">
          @if (bounces().length) {
            <optgroup label="Versions">@for (v of bounces(); track v.id) { <option [ngValue]="v.id">{{ v.label }}</option> }</optgroup>
          }
          @if (refs().length) {
            <optgroup label="References">@for (v of refs(); track v.id) { <option [ngValue]="v.id">{{ v.label }}</option> }</optgroup>
          }
        </select>
      </label>
      <button class="primary" (click)="store.runCompare()" [disabled]="store.comparing() || store.compareWith() === null">
        {{ store.comparing() ? 'Comparing…' : 'Compare' }}
      </button>
    </div>

    }

    @if (!store.otherVersions().length) {
      <div class="first">
        <h2>Nothing to compare yet</h2>
        <p class="muted">Add another bounce of this song to see what changed, second by second, or add a finished reference track to compare level and tonal balance against.</p>
        <div class="first-actions">
          <button class="primary" (click)="store.pickFiles()">Add files</button>
          <button (click)="store.pickFiles('reference')">Add reference</button>
        </div>
      </div>
    } @else if (store.comparing()) {
      <p class="empty">{{ store.comparingReference() ? 'Comparing with the reference…' : 'Aligning the files and running the null test…' }}</p>
    } @else if (rc(); as rc) {
      <p class="verdict">{{ refVerdict() }}</p>

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
        <div class="legend">
          <span><i class="sw a"></i>{{ store.version()?.label }}</span>
          <span><i class="sw b"></i>{{ ref()?.label }}</span>
        </div>
        @if (refReport(); as rr) {
          <ml-loudness-bridge [values]="cur()!.loudness.short_term" [compareValues]="rr.loudness.short_term"
            [hop]="cur()!.loudness.short_term_hop_s" [sections]="cur()!.sections" [duration]="span()" />
        }
        <ml-diff-strip [windows]="c.windows" [windowS]="c.window_s" [duration]="span()" />
        <p class="faint small">Gray: identical. Blue: small differences (often time-varying plugins). Orange and red: changed audio.</p>
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
        <ml-band-chart [a]="curBands()" [b]="refBands()" [height]="200" />
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
    :host { display: flex; flex-direction: column; gap: 22px; }
    .picker { display: flex; align-items: center; gap: 14px; }
    .first { max-width: 60ch; display: flex; flex-direction: column; gap: var(--space-xs); }
    .first-actions { display: flex; gap: var(--space-xs); margin-top: var(--space-xs); }
    .picker label { display: flex; align-items: center; gap: 8px; }
    .empty { color: var(--muted); padding: 30px 0; }
    .verdict { font-size: var(--text-subhead); max-width: 80ch; }
    .legend { display: flex; gap: 18px; font-size: var(--text-caption); margin-bottom: 4px; }
    .sw { display: inline-block; width: 10px; height: 10px; border-radius: 2px; margin-right: 6px; vertical-align: -1px; }
    .sw.a { background: var(--a); }
    .sw.b { background: var(--b); }
    .small { font-size: var(--text-caption); }
    h2 { margin-bottom: 6px; }
    .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 32px; }
    .regions .oct { font-size: var(--text-caption); }
    @container (max-width: 819px) { .cols { grid-template-columns: 1fr; } }
  `,
})
export class Compare {
  readonly store = inject(Store);
  readonly num = num;
  readonly signed = signed;
  readonly clock = clock;
  readonly abs = Math.abs;
  readonly c = computed(() => this.store.comparison());
  /** Each band's lower edge; the backend sends bands as "lo-hi". */
  readonly octaveLabels = computed(() => (this.c()?.octave_bands ?? []).map((b) => hz(Number(b.split('-')[0]))));
  readonly rc = computed(() => this.store.refComparison());
  readonly hz = hz;
  readonly bounces = computed(() => this.store.otherVersions().filter((v) => v.kind !== 'reference'));
  readonly refs = computed(() => this.store.otherVersions().filter((v) => v.kind === 'reference'));

  label(id: number) {
    return this.store.versions().find((v) => v.id === id)?.label ?? '';
  }

  readonly refVerdict = computed(() => {
    const rc = this.rc();
    if (!rc) return '';
    const d = rc.level_match_db;
    const level = Math.abs(d) < 0.5
      ? 'Both play at about the same loudness, so you can A/B them directly.'
      : `The reference is ${num(Math.abs(d))} dB ${d > 0 ? 'louder' : 'quieter'}; turn it ${d > 0 ? 'down' : 'up'} that much before comparing by ear.`;
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
  readonly cur = computed(() => this.store.report()?.report ?? null);
  readonly ref = computed(() => this.store.compareReport());
  readonly refReport = computed(() => this.store.compareReport()?.report ?? null);
  /** Shared time axis for the timeline and diff strip: the longer of the two versions. */
  readonly span = computed(() => Math.max(this.cur()?.file.duration_s ?? 0, this.refReport()?.file.duration_s ?? 0, 1));
  readonly curBands = computed(() => (this.c()?.third_octave_delta ?? []).map((d) => ({ fc: d.fc, level_db: 0, dev_db: d.dev_b })));
  readonly refBands = computed(() => (this.c()?.third_octave_delta ?? []).map((d) => ({ fc: d.fc, level_db: 0, dev_db: d.dev_a })));

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
