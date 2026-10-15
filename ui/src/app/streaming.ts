import { Component, OnInit, computed, inject, input, signal } from '@angular/core';
import { PlatformResult, Severity } from './api';
import { hostWidth } from './dom';
import { num, signed } from './fmt';
import { Sev } from './severity';
import { Store } from './store';

/** How far each platform turns the track up or down; hatched where a boost isn't applied. */
@Component({
  selector: 'ml-gain-chart',
  template: `
    <svg [attr.width]="w()" [attr.height]="height()" role="img" aria-label="Volume change on each platform">
      <defs>
        <pattern id="gain-hatch" patternUnits="userSpaceOnUse" width="5" height="5" patternTransform="rotate(45)">
          <line x1="0" y1="0" x2="0" y2="5" class="hatch" />
        </pattern>
      </defs>
      @for (t of ticks(); track t.db) {
        <line [attr.x1]="t.x" [attr.x2]="t.x" y1="0" [attr.y2]="plotH()" class="grid" [class.zero]="t.db === 0" />
        <text [attr.x]="t.x" [attr.y]="plotH() + 14" class="axis" text-anchor="middle">{{ t.label }}</text>
      }
      @for (r of bars(); track r.id) {
        <g class="row">
          <title>{{ r.title }}</title>
          <rect [attr.x]="0" [attr.y]="r.y - rowH / 2" [attr.width]="w()" [attr.height]="rowH" class="hit" />
          <text [attr.x]="labelW - 10" [attr.y]="r.y + 4" class="name" text-anchor="end">{{ r.name }}</text>
          @if (r.missW > 0) {
            <rect [attr.x]="r.missX" [attr.y]="r.y - 5" [attr.width]="r.missW" height="10" rx="2" class="miss" />
          }
          @if (r.barW > 0) {
            <rect [attr.x]="r.barX" [attr.y]="r.y - 5" [attr.width]="r.barW" height="10" rx="2" class="gain" />
          }
          <text [attr.x]="w() - 4" [attr.y]="r.y + 4" class="value" text-anchor="end">{{ r.value }}</text>
        </g>
      }
    </svg>
  `,
  styles: `
    :host { display: block; width: 100%; }
    svg { display: block; overflow: visible; }
    .grid { stroke: var(--line-soft); }
    .grid.zero { stroke: var(--muted); }
    .axis { fill: var(--faint); font-size: var(--text-caption); }
    .name { fill: var(--ink); font-size: var(--text-caption); }
    .value { fill: var(--muted); font-size: var(--text-caption); }
    .hit { fill: transparent; }
    .row:hover .hit { fill: var(--raised); }
    .gain { fill: var(--gain); }
    .miss { fill: url(#gain-hatch); stroke: var(--faint); stroke-width: 1; }
    .hatch { stroke: var(--muted); stroke-width: 1.5; }
  `,
})
export class GainChart {
  readonly rows = input.required<PlatformResult[]>();
  readonly labelW = 110;
  readonly valueW = 132;
  readonly rowH = 26;
  readonly w = hostWidth(700, 360);

  readonly plotH = computed(() => this.rows().length * this.rowH);
  readonly height = computed(() => this.plotH() + 20);

  /** Covers only the side(s) of 0 dB the changes reach, in whole steps; a track that's only turned up gets no empty negative half. */
  private readonly scale = computed(() => {
    const dbs = this.rows().flatMap((r) => [r.playback.wanted_gain_db, r.playback.gain_db]);
    const lo = Math.min(0, ...dbs);
    const hi = Math.max(0, ...dbs);
    const span = hi - lo;
    const step = span > 24 ? 6 : span > 12 ? 3 : span > 4 ? 2 : 1;
    const round = (db: number) => Math.ceil(Math.abs(db) / step - 0.01) * step;
    const min = -round(lo);
    const max = round(hi) || (min === 0 ? step : 0);
    return { min, max, step };
  });
  private x(db: number) {
    const { min, max } = this.scale();
    const left = this.labelW;
    const right = this.w() - this.valueW;
    return left + ((db - min) / (max - min)) * (right - left);
  }

  readonly ticks = computed(() => {
    const { min, max, step } = this.scale();
    const dbs: number[] = [];
    for (let db = min; db <= max; db += step) dbs.push(db);
    return dbs.map((db) => ({ db, x: this.x(db), label: db === 0 ? '0' : `${signed(db, 0)} dB` }));
  });

  readonly bars = computed(() =>
    this.rows().map((r, i) => {
      const pb = r.playback;
      const y = i * this.rowH + this.rowH / 2;
      const x0 = this.x(0);
      const xg = this.x(pb.gain_db);
      const xw = this.x(pb.wanted_gain_db);
      const short = pb.wanted_gain_db - pb.gain_db > 0.05;
      const value = short
        ? pb.gain_db > 0.05 ? `${signed(pb.gain_db)} of ${signed(pb.wanted_gain_db)} dB` : `0 (needs ${signed(pb.wanted_gain_db)} dB)`
        : Math.abs(pb.gain_db) < 0.05 ? 'unchanged' : `${signed(pb.gain_db)} dB`;
      return {
        id: r.id,
        name: r.name,
        y,
        barX: Math.min(x0, xg),
        barW: Math.abs(xg - x0),
        missX: Math.min(xg, xw),
        missW: short ? Math.abs(xw - xg) : 0,
        value,
        title: `${r.name}: ${r.summary} Plays at ${num(pb.playback_lufs)} LUFS (target ${num(pb.target_lufs, 0)}).`,
      };
    }),
  );
}

@Component({
  selector: 'ml-streaming',
  imports: [GainChart, Sev],
  template: `
    @if (s(); as s) {
      @if (verdict(); as v) {
        <header class="verdict">
          <h2><ml-sev [sev]="v.sev" />{{ v.title }}</h2>
          <p>{{ v.detail }}</p>
        </header>
      }

      <section>
        <div class="head">
          <h2>Volume on each platform</h2>
          <p class="muted">At default listener settings. Hover a row for the reason.</p>
          <div class="legend">
            @if (legend().applied) { <span><i class="sw gain"></i>Volume change</span> }
            @if (legend().short) {
              <span title="The platform never turns tracks up, or the boost would push peaks past its ceiling."><i class="sw miss"></i>Boost not applied</span>
            }
          </div>
        </div>
        <ml-gain-chart [rows]="s.platforms" />
      </section>

      <section>
        <h2>Platforms</h2>
        <table class="data platforms">
          <thead>
            <tr>
              <th class="sev"><span class="sr-only">Status</span></th><th>Platform</th><th>Normalizes</th><th class="n">Target</th>
              <th class="n">Ceiling</th><th class="n">Plays at</th><th class="n">Peak heard</th>
            </tr>
          </thead>
          @for (p of s.platforms; track p.id) {
            <tbody>
              <tr>
                <td class="sev"><ml-sev [sev]="rowSev(p)" /></td>
                <td>
                  <button class="disclose" [attr.aria-expanded]="open().has(p.id)" (click)="toggle(p.id)">
                    <svg class="chev" width="8" height="8" viewBox="0 0 8 8" aria-hidden="true"><path d="M2.5 1 5.5 4 2.5 7" /></svg>{{ p.name }}
                  </button>
                  @if (p.confidence === 'reported') { <span class="faint" title="Reported figures">&dagger;</span> }
                  @if (p.id === mainId()) { <span class="faint main">main</span> }
                </td>
                <td>{{ p.normalization === 'up_and_down' ? 'Up and down' : 'Down only' }}</td>
                <td class="n">{{ num(p.playback.target_lufs, 0) }} <span class="unit">LUFS</span></td>
                <td class="n">{{ num(p.ceiling_dbtp, 0) }} <span class="unit">dBTP</span></td>
                <td class="n">{{ num(p.playback.playback_lufs) }} <span class="unit">LUFS</span></td>
                <td class="n" [class.sev-error]="p.peak_after_dbtp > 0" [class.sev-warn]="p.peak_after_dbtp > -0.5 && p.peak_after_dbtp <= 0">
                  {{ p.peak_after_dbtp > 0 ? signed(p.peak_after_dbtp) : num(p.peak_after_dbtp) }} <span class="unit">dBTP</span></td>
              </tr>
              @for (c of problems(p); track c.code) {
                <tr class="sub"><td></td><td colspan="6"><p><strong>{{ c.title }}</strong> {{ c.detail }}</p></td></tr>
              }
              @if (open().has(p.id)) {
                <tr class="sub detail">
                  <td></td>
                  <td colspan="6">
                    <p>{{ p.summary }}</p>
                    @for (c of notes(p); track c.code) { <p><strong>{{ c.title }}</strong> {{ c.detail }}</p> }
                    @if (p.modes.length) {
                      <table class="data modes">
                        <thead><tr><th>Listener setting</th><th class="n">Target</th><th class="n">Change</th><th class="n">Plays at</th><th class="n">Limiter</th></tr></thead>
                        <tbody>
                          <tr><td>Normal (default)</td><td class="n">{{ num(p.playback.target_lufs, 0) }}</td><td class="n">{{ signed(p.playback.gain_db) }} dB</td>
                            <td class="n">{{ num(p.playback.playback_lufs) }}</td><td class="n faint">–</td></tr>
                          @for (m of p.modes; track m.name) {
                            <tr><td>{{ m.name }}</td><td class="n">{{ num(m.playback.target_lufs, 0) }}</td><td class="n">{{ signed(m.playback.gain_db) }} dB</td>
                              <td class="n">{{ num(m.playback.playback_lufs) }}</td>
                              <td class="n" [class.sev-info]="m.playback.limiter_db > 0.5">{{ m.playback.limiter_db > 0.05 ? num(m.playback.limiter_db) + ' dB' : '–' }}</td></tr>
                          }
                        </tbody>
                      </table>
                    }
                    @if (p.hires) { <p>Streams in hi-res lossless for eligible files.</p> }
                    @if (p.confidence === 'reported') { <p class="faint">{{ p.note }}</p> }
                  </td>
                </tr>
              }
            </tbody>
          }
        </table>
        <p class="foot faint">
          Peak heard is the {{ s.encoded ? 'worst encoded true peak' : 'true peak before encoding' }} plus the platform's volume change.
          @if (anyReported()) { &dagger; Not published by the platform; commonly reported figures. }
        </p>
      </section>

      <section>
        <h2>Clipping and delivery</h2>
        <ul class="findings">
          @for (f of general(); track f.code) {
            <li><ml-sev [sev]="f.severity" />
              <div><strong>{{ f.title }}</strong><p>{{ f.detail }}</p></div></li>
          }
          <li>
            <ml-sev [sev]="encodeSev()" />
            <div>
              @if (s.encoded) {
                <strong>Peaks after lossy encoding</strong>
                @if (store.codec(); as rows) {
                  <table class="data codecs">
                    <thead><tr><th>Encode</th><th class="n">True peak</th><th class="n">Loudness</th></tr></thead>
                    <tbody>
                      @for (c of rows; track c.codec) {
                        <tr>
                          <td>{{ c.label }}</td>
                          @if (c.error) { <td colspan="2" class="sev-error">{{ c.error }}</td> }
                          @else {
                            <td class="n" [class.sev-error]="(c.true_peak_dbtp ?? -99) > 0" [class.sev-warn]="(c.true_peak_dbtp ?? -99) > -0.5 && (c.true_peak_dbtp ?? -99) <= 0">
                              {{ num(c.true_peak_dbtp) }} dBTP</td>
                            <td class="n">{{ num(c.integrated_lufs) }} LUFS</td>
                          }
                        </tr>
                      }
                    </tbody>
                  </table>
                } @else {
                  <!-- Checked earlier (or by the assistant): the report keeps each platform's worst encode, not every row. -->
                  <p>Worst encoded peak {{ signed(worstEncoded()) }} dBTP. Run again for each encode's numbers.</p>
                }
              } @else {
                <strong>Lossy streams not checked</strong>
                <p>Encodes to Ogg, AAC, Opus and MP3 with ffmpeg and measures the peaks listeners hear.</p>
              }
            </div>
            <button (click)="store.runCodec()" [disabled]="store.codecBusy()">
              {{ store.codecBusy() ? 'Encoding…' : s.encoded ? 'Run again' : 'Run check' }}
            </button>
          </li>
        </ul>
      </section>
    } @else {
      <p class="muted">Working out how each platform will play this version…</p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--space-l); }
    h2 { margin-bottom: var(--space-xs); }
    .verdict h2 { font-size: var(--text-title); display: flex; align-items: center; gap: var(--space-xs); margin: 0; }
    .verdict p { margin: 2px 0 0 18px; max-width: 80ch; }
    .head { display: flex; align-items: baseline; flex-wrap: wrap; column-gap: var(--space-s); margin-bottom: var(--space-xs); }
    .head h2 { margin: 0; }
    .head p { font-size: var(--text-caption); }
    .legend { margin-left: auto; display: flex; gap: 14px; font-size: var(--text-caption); color: var(--muted); }
    .sw { display: inline-block; width: 12px; height: 10px; border-radius: 2px; margin-right: 6px; vertical-align: -1px; }
    .sw.gain { background: var(--gain); }
    .sw.miss { border: 1px solid var(--faint); background: repeating-linear-gradient(45deg, var(--muted) 0 1.5px, transparent 1.5px 4px); }
    .platforms > tbody { border-bottom: 1px solid var(--line-soft); }
    .platforms > tbody > tr > td { border-bottom: none; }
    .platforms td.n { font-variant-numeric: tabular-nums; white-space: nowrap; }
    /* Sized to its content so each value sits near its platform name, not spread across a wide window. */
    .platforms { width: auto; max-width: 100%; }
    .platforms > thead > tr > th, .platforms > tbody > tr > td { padding-right: var(--space-l); }
    .platforms > thead > tr > th.sev, .platforms > tbody > tr > td.sev { padding-right: var(--space-xs); }
    .platforms .sev { width: 10px; }
    .unit { font-size: var(--text-caption); color: var(--faint); }
    .main { font-size: var(--text-caption); margin-left: var(--space-2xs); }
    .disclose { background: none; border: none; padding: 0; min-height: 0; color: var(--ink); font-weight: 600; display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
    .disclose:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 2px; }
    .disclose:active:not(:disabled) { background: none; }
    .chev { fill: none; stroke: var(--muted); stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; transition: transform 150ms cubic-bezier(0.25, 1, 0.5, 1); }
    .disclose[aria-expanded='true'] .chev { transform: rotate(90deg); }
    .sub > td { padding-top: 0; }
    .sub p { color: var(--muted); max-width: 80ch; }
    .sub p + p, .sub p + table, .sub table + p { margin-top: var(--space-2xs); }
    .sub strong { color: var(--ink); font-weight: 600; }
    .detail { font-size: var(--text-caption); }
    .modes { max-width: 520px; }
    .foot { font-size: var(--text-caption); margin-top: var(--space-xs); max-width: 80ch; }
    .findings { list-style: none; margin: 0; padding: 0; max-width: 80ch; }
    .findings li { display: grid; grid-template-columns: 10px 1fr auto; column-gap: var(--space-xs); align-items: start; padding: var(--space-xs) 0; border-bottom: 1px solid var(--line-soft); }
    .findings p { color: var(--muted); }
    .findings ml-sev { margin-top: 5px; }
    .codecs { max-width: 360px; margin-top: var(--space-2xs); font-size: var(--text-caption); }
    @media (prefers-reduced-motion: reduce) { .chev { transition: none; } }
  `,
})
export class Streaming implements OnInit {
  readonly store = inject(Store);
  readonly num = num;
  readonly signed = signed;
  readonly s = computed(() => this.store.streaming());
  readonly mainId = computed(() => this.store.mainPlatform()?.id ?? null);
  /** Platforms whose detail row is expanded. */
  readonly open = signal<ReadonlySet<string>>(new Set());

  ngOnInit() {
    if (!this.store.streaming()) void this.store.loadStreaming();
  }

  toggle(id: string) {
    const next = new Set(this.open());
    if (!next.delete(id)) next.add(id);
    this.open.set(next);
  }

  /** The mix/reference note is said once, in the verdict. */
  readonly general = computed(() => (this.s()?.general ?? []).filter((f) => f.code !== 'mix' && f.code !== 'reference'));
  readonly anyReported = computed(() => (this.s()?.platforms ?? []).some((p) => p.confidence === 'reported'));
  readonly legend = computed(() => {
    const pbs = (this.s()?.platforms ?? []).map((p) => p.playback);
    return {
      applied: pbs.some((pb) => Math.abs(pb.gain_db) > 0.05),
      short: pbs.some((pb) => pb.wanted_gain_db - pb.gain_db > 0.05),
    };
  });

  /** A note (e.g. a loud-mode limiter) doesn't stop a platform meeting its requirements. */
  rowSev(p: PlatformResult): Severity {
    return p.status === 'info' ? 'ok' : p.status;
  }
  /** Shown under the platform's row; the table already covers what passed. */
  problems(p: PlatformResult) {
    return p.checks.filter((c) => c.severity === 'warn' || c.severity === 'error');
  }
  notes(p: PlatformResult) {
    return p.checks.filter((c) => c.severity === 'info');
  }

  /**
   * Highest true peak among all encodes. Read from the report rather than `store.codec`, which
   * only holds a check run while this version was open; between them the platforms cover every encode.
   */
  readonly worstEncoded = computed(() => {
    const peaks = (this.s()?.platforms ?? []).map((p) => p.encoded_peak_dbtp).filter((p): p is number => p !== null);
    return peaks.length ? Math.max(...peaks) : null;
  });

  readonly encodeSev = computed<Severity>(() => {
    const worst = this.worstEncoded();
    if (!this.s()?.encoded || worst === null) return 'info';
    return worst > 0 ? 'error' : worst > -0.5 ? 'warn' : 'ok';
  });

  readonly verdict = computed(() => {
    const s = this.s();
    if (!s) return null;
    const names = (ps: PlatformResult[]) =>
      ps.length === s.platforms.length ? 'every platform' : joinNames(ps.map((p) => p.name));
    const clips = s.platforms.filter((p) => p.status === 'error');
    const tight = s.platforms.filter((p) => p.status === 'warn');
    const generalError = this.general().find((f) => f.severity === 'error');
    const generalWarn = this.general().find((f) => f.severity === 'warn');

    let sev: Severity = 'ok';
    let title = s.kind === 'mix' ? 'Peaks are safe on every platform' : "Meets every platform's requirements";
    if (clips.length) [sev, title] = ['error', `Clips on ${names(clips)}`];
    else if (generalError) [sev, title] = ['error', generalError.title];
    else if (tight.length) [sev, title] = ['warn', `Peaks need attention on ${names(tight)}`];
    else if (generalWarn) [sev, title] = ['warn', generalWarn.title];

    // How loud it plays: the main platform by name, the rest as "up to".
    const it = s.kind === 'mix' ? 'This is a mix, so mastering will change these numbers. Released as is, it' : 'It';
    const main = s.platforms.find((p) => p.id === this.mainId());
    const most = Math.max(0, ...s.platforms.filter((p) => p !== main).map((p) => p.playback.quieter_by_db));
    const mainQuiet = main && main.playback.quieter_by_db >= 1 ? main.playback.quieter_by_db : 0;
    let detail: string;
    if (!main) detail = most >= 1 ? `${it} plays up to ${num(most)} dB quieter than other tracks.` : `${it} plays level with other tracks on every platform.`;
    else if (mainQuiet && most >= 1) detail = `${it} plays ${num(mainQuiet)} dB quieter than other tracks on ${main.name}, and up to ${num(most)} dB quieter elsewhere.`;
    else if (mainQuiet) detail = `${it} plays ${num(mainQuiet)} dB quieter than other tracks on ${main.name}, and level elsewhere.`;
    else if (most >= 1) detail = `${it} plays level with other tracks on ${main.name}, but up to ${num(most)} dB quieter elsewhere.`;
    else detail = `${it} plays level with other tracks on every platform.`;
    return { sev, title, detail };
  });
}

/** "Spotify, Apple Music and Tidal". */
function joinNames(names: string[]) {
  return names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names.at(-1)}` : names[0];
}
