import { Component, computed, input, output, signal } from '@angular/core';
import { PeakKind, PeakMark, Section } from './api';
import { hostWidth } from './dom';
import { clock, num, signed, time } from './fmt';

const MARK_LABEL: Record<PeakKind, string> = {
  over_full_scale: 'Above 0 dBFS',
  clipping: 'Clipped',
  true_peak: 'True peak over ceiling',
};

interface Series { values: number[]; color: 'a' | 'b' }

/** Short-term loudness across the whole song, with sections shaded. The app's signature view. */
@Component({
  selector: 'ml-loudness-bridge',
  template: `
    <svg [attr.width]="w()" [attr.height]="H" (mousemove)="move($event)" (mouseleave)="hover.set(null)" (click)="seekTo($event)" [class.seekable]="seekable()" [attr.data-focus]="focus()" [attr.role]="clickable() ? 'group' : 'img'"
         [attr.aria-label]="ariaLabel()">
      @for (s of sectionRects(); track $index) {
        <g class="sec" [class.selected]="selected() === $index" [class.clickable]="clickable()" (click)="pick($index)"
           [attr.tabindex]="s.focusable ? 0 : null" [attr.role]="s.focusable ? 'button' : null"
           [attr.aria-label]="s.focusable ? s.name : null" [attr.aria-pressed]="s.focusable ? selected() === $index : null"
           (keydown.enter)="pick($index)" (keydown.space)="$event.preventDefault(); pick($index)">
          <rect [attr.x]="s.x" y="0" [attr.width]="s.w" [attr.height]="top + plotH" [attr.class]="'band ' + s.kind" />
          @if ($index > 0) { <line [attr.x1]="s.x" [attr.x2]="s.x" y1="0" [attr.y2]="top + plotH" class="edge" /> }
          @if (s.w > 54) {
            <text [attr.x]="s.x + 6" y="13" class="sec-label">{{ s.label }}</text>
            <text [attr.x]="s.x + 6" y="27" class="sec-value">{{ s.value }}</text>
          }
        </g>
      }
      @for (g of grid(); track g.db) {
        <line [attr.x1]="left" [attr.x2]="w()" [attr.y1]="g.y" [attr.y2]="g.y" class="grid" />
        <text [attr.x]="left - 6" [attr.y]="g.y + 4" class="axis" text-anchor="end">{{ g.db }}</text>
      }
      @for (p of paths(); track p.color) {
        <path [attr.d]="p.area" [attr.class]="'area ' + p.color" />
        <path [attr.d]="p.line" [attr.class]="'trace ' + p.color" />
      }
      @if (integrated() !== null) {
        <line [attr.x1]="left" [attr.x2]="w()" [attr.y1]="y(integrated()!)" [attr.y2]="y(integrated()!)" class="ref" />
        <text [attr.x]="w() - 4" [attr.y]="y(integrated()!) - 5" class="ref-label" text-anchor="end">Integrated {{ num(integrated()) }}{{ onTarget() ? ', on target' : '' }}</text>
      }
      @if (target() !== null && !onTarget()) {
        <line [attr.x1]="left" [attr.x2]="w()" [attr.y1]="y(target()!)" [attr.y2]="y(target()!)" class="target" />
        <text [attr.x]="left + 4" [attr.y]="y(target()!) - 5" class="target-label">Streaming target {{ num(target(), 0) }}</text>
      }
      @for (m of markItems(); track $index) {
        <g class="mark" [attr.data-sev]="m.sev" [class.selected]="selectedMark() === $index" role="button" tabindex="0"
           [attr.aria-label]="m.name" [attr.aria-pressed]="selectedMark() === $index"
           (click)="$event.stopPropagation(); markClick.emit($index)"
           (keydown.enter)="markClick.emit($index)" (keydown.space)="$event.preventDefault(); markClick.emit($index)">
          <rect [attr.x]="m.x" [attr.y]="top" [attr.width]="m.w" [attr.height]="plotH" class="rule" />
          <path [attr.d]="m.head" class="head" />
          <rect [attr.x]="m.cx - 7" [attr.y]="top - 3" width="14" height="16" class="hit" />
        </g>
      }
      @if (pinned(); as p) {
        <g class="pin" [attr.transform]="'translate(' + p.tx + ',' + (top + plotH - 56) + ')'">
          <rect [attr.width]="pinW" height="50" rx="4" class="tip" />
          <text x="8" y="15" class="tip-t">{{ p.time }}</text>
          <text x="8" y="29" class="tip-v" [attr.data-sev]="p.sev">{{ p.label }}</text>
          <text x="8" y="44" class="tip-t">{{ p.peak }} dBTP true peak</text>
        </g>
      }
      @for (t of ticks(); track t.s) {
        <text [attr.x]="t.x" [attr.y]="H - 4" class="axis" text-anchor="middle">{{ t.label }}</text>
      }
      @if (playhead(); as ph) {
        <line [attr.x1]="playheadX()" [attr.x2]="playheadX()" [attr.y1]="top" [attr.y2]="top + plotH" [attr.class]="'playhead ' + ph.side + (ph.live ? '' : ' idle')" />
      }
      @if (hover(); as h) {
        <line [attr.x1]="h.x" [attr.x2]="h.x" [attr.y1]="top" [attr.y2]="top + plotH" class="cursor" />
        <g [attr.transform]="'translate(' + (h.x > w() - 150 ? h.x - 142 : h.x + 8) + ',' + (top + 6) + ')'">
          <rect width="134" [attr.height]="h.b !== null ? 50 : 34" rx="4" class="tip" />
          <text x="8" y="15" class="tip-t">{{ h.time }}</text>
          <text x="8" y="29" class="tip-v a">{{ h.a }}</text>
          @if (h.b !== null) { <text x="8" y="44" class="tip-v b">{{ h.b }}</text> }
        </g>
      }
    </svg>
  `,
  styles: `
    :host { display: block; width: 100%; }
    svg { display: block; }
    .band { fill: transparent; }
    text, .grid, .area, .trace, .ref, .target, .cursor, .tip { pointer-events: none; }
    .edge { stroke: var(--line); stroke-width: 1; pointer-events: none; }
    .band.silence { fill: color-mix(in srgb, var(--err) 16%, transparent); }
    .sec.clickable { cursor: pointer; }
    .sec.clickable:hover .band { stroke: var(--line); }
    .sec.selected .band { stroke: var(--a); stroke-width: 1.5; }
    .sec:focus { outline: none; }
    .sec:focus-visible .band { stroke: var(--focus); stroke-width: 2; }
    .sec-label { fill: var(--muted); font-size: var(--text-caption); }
    .sec-value { fill: var(--ink); font-size: var(--text-caption); font-weight: 600; }
    .grid { stroke: var(--line-soft); stroke-width: 1; }
    .axis { fill: var(--faint); font-size: var(--text-caption); }
    .area.a { fill: color-mix(in srgb, var(--a) 22%, transparent); }
    .area.b { fill: color-mix(in srgb, var(--b) 12%, transparent); }
    .trace { fill: none; stroke-width: 1.6; stroke-linejoin: round; }
    .trace.a { stroke: var(--a); }
    .trace.b { stroke: var(--b); }
    .ref { stroke: var(--ink); stroke-dasharray: 5 4; opacity: 0.55; }
    .ref-label { fill: var(--ink); font-size: var(--text-caption); opacity: 0.8; }
    .target { stroke: var(--ok); stroke-dasharray: 2 4; opacity: 0.8; }
    .target-label { fill: var(--ok); font-size: var(--text-caption); }
    .cursor { stroke: var(--ink); opacity: 0.4; }
    .seekable { cursor: pointer; }
    .playhead { stroke-width: 1.5; pointer-events: none; }
    .playhead.a { stroke: var(--a); }
    .playhead.b { stroke: var(--b); }
    .playhead.idle { opacity: 0.5; }
    /* While A/B playing, the side you can't hear steps back, so eye and ear agree. */
    .area, .trace { transition: opacity 120ms cubic-bezier(0.25, 1, 0.5, 1); }
    svg[data-focus='a'] .area.b, svg[data-focus='a'] .trace.b,
    svg[data-focus='b'] .area.a, svg[data-focus='b'] .trace.a { opacity: 0.35; }
    @media (prefers-reduced-motion: reduce) { .area, .trace { transition: none; } }
    .tip { fill: var(--raised); stroke: var(--line); }
    .tip-t { fill: var(--muted); font-size: var(--text-caption); }
    .tip-v { font-size: var(--text-caption); font-weight: 600; }
    .tip-v.a { fill: var(--a); }
    .tip-v.b { fill: var(--b); }
    .mark { color: var(--err); cursor: pointer; }
    .mark[data-sev='warn'] { color: var(--warn); }
    .mark .rule { fill: currentColor; opacity: 0.28; pointer-events: none; }
    .mark.selected .rule { opacity: 0.75; }
    .mark .head { fill: currentColor; pointer-events: none; }
    .mark .hit { fill: transparent; }
    .mark:focus { outline: none; }
    .mark:focus-visible .hit { stroke: var(--focus); stroke-width: 2; }
    .pin { pointer-events: none; }
    .tip-v[data-sev='error'] { fill: var(--err); }
    .tip-v[data-sev='warn'] { fill: var(--warn); }
  `,
})
export class LoudnessBridge {
  readonly values = input.required<number[]>();
  readonly hop = input<number>(0.5);
  readonly compareValues = input<number[] | null>(null);
  readonly sections = input<Section[]>([]);
  readonly integrated = input<number | null>(null);
  readonly target = input<number | null>(null);
  readonly duration = input<number>(0);
  readonly selected = input<number | null>(null);
  readonly clickable = input<boolean>(false);
  readonly sectionClick = output<number>();
  /** Clipping and true-peak overs, drawn as markers that can be pinned to read their time. */
  readonly marks = input<PeakMark[]>([]);
  readonly selectedMark = input<number | null>(null);
  readonly markClick = output<number>();
  /** The A/B player's position (s), drawn in the audible side's colour, faint while stopped; clicking the plot asks to play from there. */
  readonly playhead = input<{ t: number; side: 'a' | 'b'; live: boolean } | null>(null);
  /** The series being heard; the other is dimmed. */
  readonly focus = input<'a' | 'b' | null>(null);
  readonly seekable = input<boolean>(false);
  readonly seek = output<number>();
  /** Integrated sits on the target line: draw one line, not two overlapping ones. */
  readonly onTarget = computed(() => {
    const i = this.integrated();
    const t = this.target();
    return i !== null && t !== null && Math.abs(i - t) < 0.5;
  });

  readonly H = 214;
  readonly top = 36;
  readonly plotH = 158;
  readonly left = 34;
  readonly num = num;

  readonly w = hostWidth(900, 300);
  readonly hover = signal<{ x: number; time: string; a: string; b: string | null } | null>(null);
  /** The Compare tab has no integrated line, so its label leaves the value out. */
  readonly ariaLabel = computed(() => {
    const i = this.integrated();
    return 'Short-term loudness over time' + (i === null ? '' : `. Integrated ${num(i)} LUFS`);
  });

  private readonly range = computed(() => {
    const all = [...this.values(), ...(this.compareValues() ?? [])].filter((v) => v > -100);
    const max = all.length ? Math.max(...all) : -10;
    const top = Math.min(0, Math.ceil((max + 3) / 6) * 6);
    return { top, bottom: top - 42 };
  });

  private readonly span = computed(() => Math.max(this.duration(), this.values().length * this.hop(), 1));

  y(db: number): number {
    const { top, bottom } = this.range();
    const c = Math.max(bottom, Math.min(top, db));
    return this.top + ((top - c) / (top - bottom)) * this.plotH;
  }

  x(t: number): number {
    return this.left + (t / this.span()) * (this.w() - this.left);
  }

  /** Time at the centre of short-term window i (each value covers the 3 s ending at (i+1)·hop). */
  private t(i: number): number {
    return Math.max(0, (i + 1) * this.hop() - 1.5);
  }

  readonly grid = computed(() => {
    const { top, bottom } = this.range();
    const out = [];
    for (let db = top; db >= bottom; db -= 6) out.push({ db, y: this.y(db) });
    return out;
  });

  readonly ticks = computed(() => {
    const span = this.span();
    const step = span > 360 ? 60 : span > 120 ? 30 : 15;
    const out = [];
    for (let s = 0; s <= span - step / 3; s += step) out.push({ s, x: Math.max(this.left + 12, this.x(s)), label: clock(s) });
    return out;
  });

  readonly sectionRects = computed(() =>
    this.sections().map((s) => ({
      x: this.x(s.start_s),
      w: Math.max(0, this.x(s.end_s) - this.x(s.start_s)),
      kind: s.kind,
      label: s.kind === 'loud' ? 'Loud' : s.kind === 'quiet' ? 'Quiet' : 'Silent',
      value: s.kind === 'silence' ? '' : this.x(s.end_s) - this.x(s.start_s) > 96 ? `${num(s.lufs)} LUFS` : num(s.lufs),
      // Keyboard and screen-reader access to the same "measure this section" click.
      focusable: this.clickable() && s.kind !== 'silence',
      name: `Measure ${s.kind} section, ${clock(s.start_s)} to ${clock(s.end_s)}, ${num(s.lufs)} LUFS`,
    })),
  );

  readonly pinW = 176;

  readonly markItems = computed(() =>
    this.marks().map((m) => {
      const x = this.x(m.start_s);
      const w = Math.max(1.5, this.x(m.end_s) - x);
      const cx = x + w / 2;
      const t = this.top;
      // Same shapes as the severity markers: diamond for a problem, triangle for a check.
      const head = m.severity === 'error' ? `M${cx},${t}l4.5,4.5l-4.5,4.5l-4.5,-4.5z` : `M${cx - 4.5},${t}h9l-4.5,8z`;
      return { x, w, cx, head, sev: m.severity, name: `${MARK_LABEL[m.kind]} at ${time(m.start_s)}, ${signed(m.peak_db)} dBTP` };
    }),
  );

  /** Details of the pinned marker, at the foot of the plot so the hover readout stays clear. */
  readonly pinned = computed(() => {
    const i = this.selectedMark();
    const m = i === null ? undefined : this.marks()[i];
    const item = i === null ? undefined : this.markItems()[i];
    if (!m || !item) return null;
    const long = m.end_s - m.start_s > 0.25;
    return {
      tx: Math.min(Math.max(this.left + 4, item.cx - this.pinW / 2), this.w() - this.pinW - 4),
      time: long ? `${time(m.start_s)} to ${time(m.end_s)}` : time(m.start_s),
      label: MARK_LABEL[m.kind],
      peak: signed(m.peak_db),
      sev: m.severity,
    };
  });

  readonly paths = computed(() => {
    const series: Series[] = [];
    const cmp = this.compareValues();
    if (cmp) series.push({ values: cmp, color: 'b' });
    series.push({ values: this.values(), color: 'a' });
    // The series being heard draws on top.
    if (this.focus() === 'b') series.reverse();
    const base = this.top + this.plotH;
    return series.map((s) => {
      const pts = s.values.map((v, i) => `${this.x(this.t(i)).toFixed(1)},${this.y(v).toFixed(1)}`);
      const line = 'M' + pts.join('L');
      const area = `M${this.x(this.t(0)).toFixed(1)},${base}L${pts.join('L')}L${this.x(this.t(s.values.length - 1)).toFixed(1)},${base}Z`;
      return { color: s.color, line, area };
    });
  });

  move(ev: MouseEvent) {
    const rect = (ev.currentTarget as SVGElement).getBoundingClientRect();
    const px = ev.clientX - rect.left;
    if (px < this.left) return this.hover.set(null);
    const t = ((px - this.left) / (this.w() - this.left)) * this.span();
    const i = Math.round((t + 1.5) / this.hop() - 1);
    const vals = this.values();
    if (i < 0 || i >= vals.length) return this.hover.set(null);
    const cmp = this.compareValues();
    const fmt = (v: number | undefined) => (v === undefined || v <= -100 ? 'silent' : `${num(v)} LUFS`);
    this.hover.set({ x: px, time: clock(t), a: fmt(vals[i]), b: cmp ? fmt(cmp[i]) : null });
  }

  readonly playheadX = computed(() => {
    const p = this.playhead();
    return p === null ? 0 : this.x(Math.min(p.t, this.span()));
  });

  seekTo(ev: MouseEvent) {
    if (!this.seekable()) return;
    const rect = (ev.currentTarget as SVGElement).getBoundingClientRect();
    const px = ev.clientX - rect.left;
    if (px >= this.left) this.seek.emit(((px - this.left) / (this.w() - this.left)) * this.span());
  }

  pick(i: number) {
    if (this.clickable() && this.sections()[i]?.kind !== 'silence') this.sectionClick.emit(i);
  }
}
