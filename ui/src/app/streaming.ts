import { Component, ElementRef, OnDestroy, OnInit, computed, inject, input, signal } from '@angular/core';
import { Finding, PlatformResult } from './api';
import { num, signed } from './fmt';
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
            <rect [attr.x]="r.barX" [attr.y]="r.y - 5" [attr.width]="r.barW" height="10" rx="2" [class.up]="r.up" [class.down]="!r.up" />
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
    .axis { fill: var(--faint); font-size: 11px; }
    .name { fill: var(--ink); font-size: 12px; }
    .value { fill: var(--muted); font-size: 12px; }
    .hit { fill: transparent; }
    .row:hover .hit { fill: var(--raised); }
    .up { fill: var(--gain-up); }
    .down { fill: var(--gain-down); }
    .miss { fill: url(#gain-hatch); stroke: var(--faint); stroke-width: 1; }
    .hatch { stroke: var(--muted); stroke-width: 1.5; }
  `,
})
export class GainChart implements OnInit, OnDestroy {
  readonly rows = input.required<PlatformResult[]>();
  readonly labelW = 110;
  readonly valueW = 132;
  readonly rowH = 26;
  private readonly el = inject(ElementRef<HTMLElement>);
  private ro?: ResizeObserver;
  readonly w = signal(700);

  ngOnInit() {
    this.ro = new ResizeObserver((e) => this.w.set(Math.max(360, Math.floor(e[0].contentRect.width))));
    this.ro.observe(this.el.nativeElement);
  }
  ngOnDestroy() {
    this.ro?.disconnect();
  }

  readonly plotH = computed(() => this.rows().length * this.rowH);
  readonly height = computed(() => this.plotH() + 20);

  /** Symmetric scale in 3 dB steps, wide enough for the largest wanted change. */
  private readonly maxDb = computed(() => {
    const m = Math.max(3, ...this.rows().map((r) => Math.max(Math.abs(r.playback.wanted_gain_db), Math.abs(r.playback.gain_db))));
    return Math.ceil(m / 3) * 3;
  });
  private x(db: number) {
    const left = this.labelW;
    const right = this.w() - this.valueW;
    return left + ((db + this.maxDb()) / (2 * this.maxDb())) * (right - left);
  }

  readonly ticks = computed(() => {
    const m = this.maxDb();
    return [-m, -m / 2, 0, m / 2, m].map((db) => ({ db, x: this.x(db), label: db === 0 ? '0' : `${signed(db, 0)} dB` }));
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
        up: pb.gain_db > 0,
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
  imports: [GainChart],
  template: `
    @if (s(); as s) {
      <p class="verdict">{{ verdict() }}</p>

      <section>
        <div class="chart-head">
          <h2>Volume on each platform</h2>
          <div class="legend">
            <span><i class="sw up"></i>Turned up</span>
            <span><i class="sw down"></i>Turned down</span>
            <span><i class="sw miss"></i>Needed but not applied</span>
          </div>
        </div>
        <p class="muted lead">How far each platform changes the volume to match its loudness target, with default listening settings. A hatched bar is the boost the platform won't give: it either never turns tracks up, or would push peaks past its ceiling.</p>
        <ml-gain-chart [rows]="s.platforms" />
      </section>

      <div class="cols">
        <section>
          <h2>Clipping and delivery</h2>
          <ul class="findings">
            @for (f of s.general; track f.code) {
              <li [attr.data-sev]="f.severity"><span class="dot" aria-hidden="true"></span>
                <div><strong>{{ f.title }}</strong><p>{{ f.detail }}</p></div></li>
            }
          </ul>
        </section>
        <section class="codec">
          <div class="codec-head">
            <h2>Encode check</h2>
            <button (click)="store.runCodec()" [disabled]="store.codecBusy()">
              {{ store.codecBusy() ? 'Encoding…' : store.codec() ? 'Run again' : 'Run check' }}
            </button>
          </div>
          <p class="muted">Encodes to the formats platforms stream (Ogg, AAC, Opus, MP3) with ffmpeg and measures the peaks listeners actually get. The platform cards below use these results.</p>
          @if (store.codec(); as rows) {
            <table class="data">
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
          }
        </section>
      </div>

      <section>
        <h2>Platforms</h2>
        <div class="grid">
          @for (p of s.platforms; track p.id) {
            <article class="card" [attr.data-sev]="p.status">
              <header>
                <span class="dot" aria-hidden="true"></span>
                <h3>{{ p.name }}</h3>
                <span class="status">{{ statusLabel(p) }}</span>
                @if (p.id === mainId()) { <span class="tag main">Main platform</span> }
                @if (p.confidence === 'reported') { <span class="tag" [title]="p.note">Reported figures</span> }
              </header>
              <p class="meta">{{ num(p.playback.target_lufs, 0) }} LUFS · {{ p.normalization === 'up_and_down' ? 'turns up and down' : 'turns down only' }} · ceiling {{ num(p.ceiling_dbtp, 0) }} dBTP{{ p.hires ? ' · hi-res tier' : '' }}</p>
              <p class="summary">{{ p.summary }}</p>
              <dl class="nums">
                <div><dt>Volume change</dt><dd>{{ signed(p.playback.gain_db) }} <span>dB</span></dd></div>
                <div><dt>Plays at</dt><dd>{{ num(p.playback.playback_lufs) }} <span>LUFS</span></dd></div>
                <div><dt>Peak heard</dt><dd [class.sev-error]="p.peak_after_dbtp > 0">{{ num(p.peak_after_dbtp) }} <span>dBTP</span></dd>
                  <dd class="faint small">{{ p.encoded_peak_dbtp !== null ? 'after encoding' : 'before encoding' }}</dd></div>
              </dl>
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
              <ul class="findings compact">
                @for (c of shownChecks(p.checks); track c.code) {
                  <li [attr.data-sev]="c.severity"><span class="dot" aria-hidden="true"></span>
                    <div><strong>{{ c.title }}</strong><p>{{ c.detail }}</p></div></li>
                }
              </ul>
              @if (p.confidence === 'reported') { <p class="note">{{ p.note }}</p> }
            </article>
          }
        </div>
      </section>
    } @else {
      <p class="muted">Working out how each platform will play this version…</p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: 22px; }
    h2 { margin-bottom: 6px; }
    .verdict { font-size: 16px; max-width: 80ch; }
    .lead { font-size: 12px; margin-bottom: 10px; max-width: 80ch; }
    .chart-head { display: flex; align-items: baseline; justify-content: space-between; gap: 16px; flex-wrap: wrap; }
    .legend { display: flex; gap: 14px; font-size: 12px; color: var(--muted); }
    .sw { display: inline-block; width: 12px; height: 10px; border-radius: 2px; margin-right: 6px; vertical-align: -1px; }
    .sw.up { background: var(--gain-up); }
    .sw.down { background: var(--gain-down); }
    .sw.miss { border: 1px solid var(--faint); background: repeating-linear-gradient(45deg, var(--muted) 0 1.5px, transparent 1.5px 4px); }
    .cols { display: grid; grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr); gap: 32px; }
    .findings { list-style: none; margin: 0; padding: 0; }
    .findings li { display: grid; grid-template-columns: 14px 1fr; gap: 8px; padding: 8px 0; border-bottom: 1px solid var(--line-soft); }
    .findings p { color: var(--muted); max-width: 70ch; }
    .findings.compact li { padding: 6px 0; font-size: 13px; }
    .findings.compact li:last-child { border-bottom: none; }
    .dot { width: 8px; height: 8px; border-radius: 50%; margin-top: 6px; background: var(--muted); }
    [data-sev='error'] > .dot, [data-sev='error'] > header > .dot { background: var(--err); }
    [data-sev='warn'] > .dot, [data-sev='warn'] > header > .dot { background: var(--warn); }
    [data-sev='ok'] > .dot, [data-sev='ok'] > header > .dot { background: var(--ok); }
    [data-sev='info'] > .dot, [data-sev='info'] > header > .dot { background: var(--info); }
    .codec-head { display: flex; align-items: center; justify-content: space-between; }
    .codec-head h2 { margin: 0; }
    .codec > p { font-size: 12px; margin: 4px 0 8px; }
    .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); gap: 14px; }
    .card { border: 1px solid var(--line); border-radius: var(--radius-m); background: var(--panel); padding: 12px 14px; display: flex; flex-direction: column; gap: 8px; }
    .card header { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
    .card header .dot { margin-top: 0; }
    .card h3 { color: var(--ink); font-size: 14px; }
    .status { font-size: 12px; color: var(--muted); }
    .tag { font-size: 11px; color: var(--muted); border: 1px solid var(--line); border-radius: 3px; padding: 0 5px; }
    .tag.main { color: var(--a); border-color: var(--a); }
    .meta { font-size: 12px; color: var(--faint); }
    .summary { max-width: 70ch; }
    .nums { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; margin: 0; }
    .nums dt { font-size: 11px; color: var(--faint); }
    .nums dd { margin: 0; font-size: 18px; font-family: var(--font-display); }
    .nums dd span { font-size: 12px; color: var(--muted); }
    .nums dd.small { font-size: 11px; font-family: var(--font); }
    .modes { font-size: 12px; }
    .note { font-size: 12px; color: var(--faint); }
    @media (max-width: 1100px) { .cols { grid-template-columns: 1fr; } }
  `,
})
export class Streaming implements OnInit {
  readonly store = inject(Store);
  readonly num = num;
  readonly signed = signed;
  readonly s = computed(() => this.store.streaming());
  readonly mainId = computed(() => this.store.settings()?.main_platform ?? 'spotify');

  ngOnInit() {
    if (!this.store.streaming()) void this.store.loadStreaming();
  }

  /** Requirement problems only; passing checks are summarized by the status label. */
  shownChecks(checks: Finding[]) {
    const problems = checks.filter((c) => c.severity !== 'ok');
    return problems.length ? problems : checks.filter((c) => c.code !== 'tp_ceiling');
  }

  statusLabel(p: PlatformResult) {
    return { ok: 'Meets requirements', info: 'Meets requirements', warn: 'Needs attention', error: 'Clips' }[p.status];
  }

  readonly verdict = computed(() => {
    const s = this.s();
    if (!s) return '';
    const ok = s.platforms.filter((p) => p.status === 'ok' || p.status === 'info');
    const lead = s.kind === 'mix' ? 'If released as it is, this mix would meet' : s.kind === 'reference' ? 'This reference meets' : 'Meets';
    let text = `${lead} the requirements of ${ok.length} of ${s.platforms.length} platforms.`;
    if (s.general.some((f) => f.severity === 'error')) text += ' It clips, so fix that before anything else.';
    const main = s.platforms.find((p) => p.id === this.mainId());
    if (main) text += ` ${main.summary}`;
    const quiet = s.platforms.filter((p) => p.id !== main?.id && p.playback.quieter_by_db >= 1);
    if (quiet.length) {
      const names = quiet.map((p) => p.name);
      const list = names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names.at(-1)}` : names[0];
      const most = Math.max(...quiet.map((p) => p.playback.quieter_by_db));
      text += ` On ${list} it plays quieter than other tracks (by up to ${num(most)} dB).`;
    }
    if (!s.encoded) text += ' Run the encode check to include lossy-stream peaks.';
    return text;
  });
}
