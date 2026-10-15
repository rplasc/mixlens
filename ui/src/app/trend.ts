import { Component, computed, inject, signal } from '@angular/core';
import { VersionSummary } from './api';
import { hostWidth } from './dom';
import { num, signed } from './fmt';
import { Store } from './store';

type Metric = 'integrated_lufs' | 'true_peak_dbtp' | 'plr_db' | 'correlation';

interface MetricSpec {
  key: Metric;
  label: string;
  unit: string;
  digits: number;
  /** Smallest y range drawn, so jitter between bounces doesn't read as a swing. */
  minSpan: number;
  /** A change between two bounces at least this large is worth naming. */
  notable: number;
}

const METRICS: MetricSpec[] = [
  { key: 'integrated_lufs', label: 'Integrated', unit: 'LUFS', digits: 1, minSpan: 2, notable: 0.5 },
  { key: 'true_peak_dbtp', label: 'True peak', unit: 'dBTP', digits: 1, minSpan: 2, notable: 0.5 },
  { key: 'plr_db', label: 'PLR', unit: 'dB', digits: 1, minSpan: 2, notable: 1 },
  { key: 'correlation', label: 'Correlation', unit: '', digits: 2, minSpan: 0.2, notable: 0.05 },
];

/** Reports from before a field existed (or preview data) leave it out. */
const has = (x: number | undefined): x is number => typeof x === 'number' && isFinite(x);
const issues = (v: VersionSummary) => v.errors + v.warnings;

/** Cut from the middle: bounces of one song usually differ at the end ("…v3", "…final"). */
function shorten(label: string, chars: number): string {
  if (label.length <= chars) return label;
  const head = Math.max(1, Math.floor((chars - 1) * 0.4));
  return label.slice(0, head).trimEnd() + '…' + label.slice(label.length - (chars - 1 - head)).trimStart();
}

/** How one measurement moved between two bounces, in a few words. */
function describe(m: Metric, d: number): string {
  const a = num(Math.abs(d), m === 'correlation' ? 2 : 1);
  switch (m) {
    case 'integrated_lufs': return `${a} LU ${d > 0 ? 'louder' : 'quieter'}`;
    case 'true_peak_dbtp': return `true peak ${d > 0 ? 'up' : 'down'} ${a} dB`;
    case 'plr_db': return `${a} dB ${d > 0 ? 'more dynamic' : 'more compressed'} (PLR)`;
    case 'correlation': return `correlation ${d > 0 ? 'up' : 'down'} ${a}`;
  }
}

/**
 * Whether the song is converging or regressing, from the bounces of the latest one's type
 * (mixes and masters are checked by different rules). At most two sentences, problems first.
 */
export function trendVerdict(list: VersionSummary[], ceiling: number): string {
  const latest = list.at(-1);
  if (!latest) return '';
  const same = list.filter((v) => v.kind === latest.kind);
  const noun = latest.kind === 'master' ? 'masters' : 'mixes';
  if (same.length < 2) {
    const others = list.length - 1;
    return `${latest.label} is the first ${latest.kind}. The ${others} ${others === 1 ? 'bounce' : 'bounces'} before it ${others === 1 ? 'is' : 'are'} checked by different rules, so compare their measurements rather than their issues.`;
  }
  const prev = same.at(-2)!;
  const out: string[] = [];

  if (latest.kind === 'master' && latest.true_peak_dbtp > ceiling && prev.true_peak_dbtp <= ceiling) {
    out.push(`${latest.label} peaks at ${signed(latest.true_peak_dbtp)} dBTP, over the ${num(ceiling)} dBTP ceiling that ${prev.label} stayed under.`);
  }
  const corr = same.filter((v) => has(v.correlation));
  const widest = corr.reduce<VersionSummary | null>((b, v) => (!b || v.correlation > b.correlation ? v : b), null);
  if (widest && has(latest.correlation) && widest.correlation - latest.correlation > 0.1) {
    out.push(`Correlation fell to ${num(latest.correlation, 2)} from ${num(widest.correlation, 2)} in ${widest.label}, so it holds up less well in mono.`);
  }

  const first = same[0];
  const [a, b] = [issues(first), issues(latest)];
  let trend: string;
  if (a === b) trend = b === 0 ? `None of the ${same.length} ${noun} has an issue.` : `Across ${same.length} ${noun}, issues held at ${b}.`;
  else trend = `Across ${same.length} ${noun}, issues ${b < a ? 'went' : 'rose'} from ${a} to ${b}.`;
  const fewest = same.reduce((x, v) => (issues(v) < issues(x) ? v : x));
  if (issues(fewest) < b && fewest !== first) trend += ` ${fewest.label} had the fewest (${issues(fewest)}); check what changed since then.`;
  out.push(trend);

  const moved = METRICS.map((m) => ({ m, d: latest[m.key] - prev[m.key] }))
    .filter((s) => has(s.d) && Math.abs(s.d) >= s.m.notable)
    .sort((x, y) => Math.abs(y.d) / y.m.notable - Math.abs(x.d) / x.m.notable);
  out.push(moved.length
    ? `Since ${prev.label}: ${moved.slice(0, 2).map((s) => describe(s.m.key, s.d)).join(', ')}.`
    : `Since ${prev.label}, loudness, peaks, PLR and correlation barely moved, so the ${latest.kind} is settling.`);

  return out.slice(0, 2).join(' ');
}

/** Each version's loudness, peaks, dynamics, stereo and issues across the song's bounces. */
@Component({
  selector: 'ml-trend',
  template: `
    <p class="verdict">{{ verdict() }}</p>
    <div class="plot">
      <svg [attr.width]="w()" [attr.height]="H" role="group" [attr.aria-label]="'Measurements across ' + list().length + ' versions'">
        @for (r of rows(); track r.spec.key) {
          <g [attr.transform]="'translate(0,' + r.y0 + ')'">
            <line [attr.x1]="0" [attr.x2]="w()" [attr.y1]="ROW" [attr.y2]="ROW" class="sep" />
            <text x="0" y="17" class="name">{{ r.spec.label }}</text>
            <text x="0" y="34" class="value">{{ r.value }}<tspan class="unit">{{ r.spec.unit ? ' ' + r.spec.unit : '' }}</tspan></text>
            <text [attr.x]="w() - 2" [attr.y]="r.yTop + 4" class="axis" text-anchor="end">{{ r.top }}</text>
            <text [attr.x]="w() - 2" [attr.y]="r.yBottom + 4" class="axis" text-anchor="end">{{ r.bottom }}</text>
            @for (t of r.lines; track t.label) {
              <line [attr.x1]="LEFT" [attr.x2]="w() - RIGHT" [attr.y1]="t.y" [attr.y2]="t.y" [attr.class]="'threshold ' + t.kind" />
              <text [attr.x]="LEFT + 4" [attr.y]="t.labelY" [attr.class]="'threshold-label ' + t.kind">{{ t.label }}</text>
            }
            @for (s of r.segments; track $index) {
              <line [attr.x1]="s.x1" [attr.y1]="s.y1" [attr.x2]="s.x2" [attr.y2]="s.y2" class="trace" [class.switch]="s.switch" />
            }
            @for (p of r.points; track p.id) {
              <circle [attr.cx]="p.x" [attr.cy]="p.y" r="3.5" [attr.class]="p.cls" />
            }
          </g>
        }
        <g [attr.transform]="'translate(0,' + issuesY + ')'">
          <text x="0" y="17" class="name">Issues</text>
          <text x="0" y="34" class="value">{{ shownIssues() }}</text>
          @for (b of bars(); track b.id) {
            @if (b.total === 0) {
              <line [attr.x1]="b.x" [attr.x2]="b.x + b.bw" [attr.y1]="ISSUES_H - 6" [attr.y2]="ISSUES_H - 6" class="none" />
            } @else {
              <rect [attr.x]="b.x" [attr.y]="b.yErr" [attr.width]="b.bw" [attr.height]="b.hErr" class="err" />
              <rect [attr.x]="b.x" [attr.y]="b.yWarn" [attr.width]="b.bw" [attr.height]="b.hWarn" class="warn" />
            }
          }
        </g>
        @for (l of xLabels(); track l.id) {
          <text [attr.x]="l.x" [attr.y]="H - 4" class="axis" [class.cur]="l.cur" text-anchor="middle">{{ l.text }}<title>{{ l.full }}</title></text>
        }
        @for (c of columns(); track c.id) {
          <rect [attr.x]="c.x" y="0" [attr.width]="c.w" [attr.height]="H - LABEL_H" class="col" [class.current]="c.current"
            role="button" tabindex="0" [attr.aria-label]="c.name" [attr.aria-pressed]="c.partner"
            (mouseenter)="hover.set(c.i)" (mouseleave)="hover.set(null)" (focus)="hover.set(c.i)" (blur)="hover.set(null)"
            (click)="pick(c.id)" (keydown.enter)="pick(c.id)" (keydown.space)="$event.preventDefault(); pick(c.id)" />
        }
      </svg>
      @if (tip(); as t) {
        <div class="tip" [style.left.px]="t.left" aria-hidden="true">
          <strong>{{ t.label }}</strong><span class="muted kind">{{ t.kind }}</span>
          @if (t.notes) { <p>{{ t.notes }}</p> }
          <p class="faint">{{ t.action }}</p>
        </div>
      }
    </div>
    <p class="faint small">Hollow: mix. Filled: master. A dashed line joins a mix and a master, which are checked by different rules. Select a version to compare it with {{ store.version()?.label }} below.</p>
  `,
  styles: `
    :host { display: block; width: 100%; }
    .verdict { font-size: var(--text-subhead); max-width: 80ch; margin-bottom: var(--space-s); }
    .plot { position: relative; }
    svg { display: block; }
    text { pointer-events: none; }
    .name { fill: var(--muted); font-size: var(--text-caption); }
    .value { fill: var(--ink); font-size: var(--text-body); font-weight: 600; font-variant-numeric: tabular-nums; }
    .unit { fill: var(--muted); font-size: var(--text-caption); font-weight: 400; }
    .axis { fill: var(--faint); font-size: var(--text-caption); }
    .axis.cur { fill: var(--a); font-weight: 600; }
    .sep { stroke: var(--line-soft); }
    .threshold { stroke-dasharray: 2 4; opacity: 0.8; }
    .threshold.target { stroke: var(--ok); }
    .threshold.ceiling { stroke: var(--err); }
    .threshold.zero { stroke: var(--muted); opacity: 0.5; stroke-dasharray: none; }
    .threshold-label { font-size: var(--text-caption); }
    .threshold-label.target { fill: var(--ok); }
    .threshold-label.ceiling { fill: var(--err); }
    .threshold-label.zero { fill: var(--faint); }
    .trace { stroke: var(--line-strong); stroke-width: 1.4; }
    .trace.switch { stroke-dasharray: 3 3; }
    .pt { fill: var(--bg); stroke: var(--muted); stroke-width: 1.5; }
    .pt.master { fill: var(--muted); }
    .pt.current { stroke: var(--a); }
    .pt.current.master { fill: var(--a); }
    .pt.partner { stroke: var(--b); }
    .pt.partner.master { fill: var(--b); }
    .err { fill: var(--err); }
    .warn { fill: var(--warn); }
    .none { stroke: var(--line-strong); stroke-width: 1.5; }
    .col { fill: transparent; cursor: pointer; }
    .col.current { cursor: default; }
    .col:hover, .col:focus-visible { fill: color-mix(in srgb, var(--ink) 5%, transparent); }
    .col:focus { outline: none; }
    .col:focus-visible { stroke: var(--focus); stroke-width: 2; }
    /* Below the plot, so it never covers the points being read. */
    .tip { position: absolute; top: calc(100% + 2px); z-index: 1; width: 220px; padding: 8px 10px; background: var(--raised); border: 1px solid var(--line);
      border-radius: var(--radius-s); font-size: var(--text-caption); pointer-events: none; }
    .tip .kind { margin-left: 6px; }
    .tip p { margin: 4px 0 0; overflow-wrap: anywhere; }
    .small { font-size: var(--text-caption); margin-top: var(--space-2xs); }
  `,
})
export class Trend {
  readonly store = inject(Store);
  readonly w = hostWidth(900, 300);
  readonly hover = signal<number | null>(null);

  readonly LEFT = 104;
  readonly RIGHT = 44;
  readonly ROW = 46;
  readonly ISSUES_H = 42;
  readonly LABEL_H = 20;
  readonly issuesY = METRICS.length * this.ROW;
  readonly H = this.issuesY + this.ISSUES_H + this.LABEL_H;

  /** Bounces only, in the order they were added. */
  readonly list = computed(() => this.store.mixVersions());
  private readonly hasMaster = computed(() => this.list().some((v) => v.kind === 'master'));
  private readonly target = computed(() => this.store.settings()?.effective_target_lufs ?? -14);
  private readonly ceiling = computed(() => {
    const id = this.store.settings()?.main_platform;
    return this.store.platforms().find((p) => p.id === id)?.ceiling_dbtp ?? -1;
  });
  readonly verdict = computed(() => trendVerdict(this.list(), this.ceiling()));

  private readonly slot = computed(() => (this.w() - this.LEFT - this.RIGHT) / Math.max(1, this.list().length));
  private x(i: number) {
    return this.LEFT + this.slot() * (i + 0.5);
  }
  /** The left column reads the hovered version, otherwise the latest. */
  private readonly shown = computed(() => this.list()[this.hover() ?? this.list().length - 1]);

  private role(v: VersionSummary) {
    return v.id === this.store.versionId() ? 'current' : v.id === this.store.compareWith() ? 'partner' : '';
  }

  readonly rows = computed(() => {
    const list = this.list();
    const shown = this.shown();
    return METRICS.map((spec, r) => {
      const lines: { label: string; kind: 'target' | 'ceiling' | 'zero'; v: number }[] = [];
      if (this.hasMaster() && spec.key === 'integrated_lufs') lines.push({ v: this.target(), kind: 'target', label: `Target ${num(this.target(), 0)}` });
      if (this.hasMaster() && spec.key === 'true_peak_dbtp') lines.push({ v: this.ceiling(), kind: 'ceiling', label: `Ceiling ${num(this.ceiling())}` });
      const vals = list.map((v) => v[spec.key]).filter(has);
      let lo = Math.min(...vals, ...lines.map((l) => l.v));
      let hi = Math.max(...vals, ...lines.map((l) => l.v));
      if (!isFinite(lo)) [lo, hi] = [-spec.minSpan / 2, spec.minSpan / 2];
      if (hi - lo < spec.minSpan) {
        const mid = (hi + lo) / 2;
        [lo, hi] = [mid - spec.minSpan / 2, mid + spec.minSpan / 2];
      }
      if (spec.key === 'correlation' && lo <= 0 && hi >= 0) lines.push({ v: 0, kind: 'zero', label: '0' });
      // Plot area inside the row, clear of the separator.
      const yTop = 8;
      const yBottom = this.ROW - 8;
      const y = (v: number) => yTop + ((hi - v) / (hi - lo)) * (yBottom - yTop);
      // Labels sit above their line, or below it when the line is at the top of the row and the label would be cut off.
      const marks = lines.map((l) => ({ ...l, y: y(l.v), labelY: y(l.v) < 16 ? y(l.v) + 12 : y(l.v) - 3 }));

      const pts = list.map((v, i) => (has(v[spec.key]) ? { id: v.id, x: this.x(i), y: y(v[spec.key]), cls: `pt ${this.role(v)}${v.kind === 'master' ? ' master' : ''}` } : null));
      const segments: { x1: number; y1: number; x2: number; y2: number; switch: boolean }[] = [];
      for (let i = 1; i < pts.length; i++) {
        const [a, b] = [pts[i - 1], pts[i]];
        if (a && b) segments.push({ x1: a.x, y1: a.y, x2: b.x, y2: b.y, switch: list[i - 1].kind !== list[i].kind });
      }
      return {
        spec, y0: r * this.ROW, yTop, yBottom, lines: marks, segments,
        points: pts.filter((p): p is NonNullable<typeof p> => p !== null),
        top: num(hi, spec.digits), bottom: num(lo, spec.digits),
        value: shown ? num(shown[spec.key], spec.digits) : '–',
      };
    });
  });

  readonly shownIssues = computed(() => {
    const v = this.shown();
    if (!v) return '–';
    if (!issues(v)) return 'None';
    const part = (n: number, w: string) => (n ? `${n} ${w}${n === 1 ? '' : 's'}` : '');
    return [part(v.errors, 'error'), part(v.warnings, 'warning')].filter(Boolean).join(', ');
  });

  readonly bars = computed(() => {
    const list = this.list();
    const max = Math.max(4, ...list.map(issues));
    const base = this.ISSUES_H - 6;
    const unit = (base - 6) / max;
    const bw = Math.max(3, Math.min(14, this.slot() * 0.4));
    return list.map((v, i) => {
      const hErr = v.errors * unit;
      const hWarn = v.warnings * unit;
      return { id: v.id, total: issues(v), x: this.x(i) - bw / 2, bw, yErr: base - hErr, hErr, yWarn: base - hErr - hWarn, hWarn };
    });
  });

  readonly xLabels = computed(() => {
    const list = this.list();
    const slot = this.slot();
    // Thin labels out when slots get narrow, always keeping the latest.
    const step = Math.max(1, Math.ceil(48 / slot));
    const chars = Math.max(3, Math.floor((slot * step) / 7));
    return list
      .map((v, i) => ({ v, i }))
      .filter(({ i }) => (list.length - 1 - i) % step === 0)
      .map(({ v, i }) => ({
        id: v.id, x: this.x(i), full: v.label, cur: v.id === this.store.versionId(),
        text: shorten(v.label, chars),
      }));
  });

  readonly columns = computed(() => {
    const slot = this.slot();
    return this.list().map((v, i) => {
      const values = METRICS.map((m) => `${m.label} ${num(v[m.key], m.digits)}${m.unit ? ' ' + m.unit : ''}`).join(', ');
      const n = issues(v);
      return {
        id: v.id, i, x: this.x(i) - slot / 2, w: slot,
        current: v.id === this.store.versionId(),
        partner: v.id === this.store.compareWith(),
        name: `${v.label}, ${v.kind}: ${values}, ${n === 1 ? '1 issue' : `${n} issues`}`,
      };
    });
  });

  readonly tip = computed(() => {
    const i = this.hover();
    const v = i === null ? undefined : this.list()[i];
    if (i === null || !v) return null;
    const current = v.id === this.store.versionId();
    const notes = v.notes.split('\n')[0].trim();
    const cx = this.x(i);
    return {
      left: cx + this.slot() / 2 + 220 > this.w() ? Math.max(0, cx - this.slot() / 2 - 224) : cx + this.slot() / 2 + 4,
      label: v.label,
      kind: v.kind,
      notes: notes.length > 120 ? notes.slice(0, 119) + '…' : notes,
      action: current ? 'The version you have open' : `Select to compare with ${this.store.version()?.label ?? 'this version'}`,
    };
  });

  pick(id: number) {
    if (id === this.store.versionId()) return;
    this.store.compareWith.set(id);
    void this.store.runCompare();
  }
}
