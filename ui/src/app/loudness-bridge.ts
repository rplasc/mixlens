import { Component, ElementRef, OnDestroy, OnInit, computed, inject, input, output, signal } from '@angular/core';
import { Section } from './api';
import { clock, num } from './fmt';

interface Series { values: number[]; hop: number; color: 'a' | 'b' }

/** Short-term loudness across the whole song, with sections shaded. The app's signature view. */
@Component({
  selector: 'ml-loudness-bridge',
  template: `
    <svg [attr.width]="w()" [attr.height]="H" (mousemove)="move($event)" (mouseleave)="hover.set(null)" role="img"
         [attr.aria-label]="'Short-term loudness over time. Integrated ' + num(integrated()) + ' LUFS'">
      @for (s of sectionRects(); track $index) {
        <g class="sec" [class.selected]="selected() === $index" [class.clickable]="clickable()" (click)="pick($index)">
          <rect [attr.x]="s.x" y="0" [attr.width]="s.w" [attr.height]="top + plotH" [attr.class]="'band ' + s.kind" />
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
        <text [attr.x]="w() - 4" [attr.y]="y(integrated()!) - 5" class="ref-label" text-anchor="end">Integrated {{ num(integrated()) }}</text>
      }
      @if (target() !== null) {
        <line [attr.x1]="left" [attr.x2]="w()" [attr.y1]="y(target()!)" [attr.y2]="y(target()!)" class="target" />
        <text [attr.x]="left + 4" [attr.y]="y(target()!) - 5" class="target-label">Streaming target {{ num(target(), 0) }}</text>
      }
      @for (t of ticks(); track t.s) {
        <text [attr.x]="t.x" [attr.y]="H - 4" class="axis" text-anchor="middle">{{ t.label }}</text>
      }
      @if (hover(); as h) {
        <line [attr.x1]="h.x" [attr.x2]="h.x" [attr.y1]="top" [attr.y2]="top + plotH" class="cursor" />
        <g [attr.transform]="'translate(' + (h.x > w() - 150 ? h.x - 142 : h.x + 8) + ',' + (top + 6) + ')'">
          <rect width="134" [attr.height]="h.b !== null ? 50 : 34" rx="4" class="tip" />
          <text x="8" y="15" class="tip-t">{{ h.time }}</text>
          <text x="8" y="29" class="tip-v a">{{ h.a }} LUFS</text>
          @if (h.b !== null) { <text x="8" y="44" class="tip-v b">{{ h.b }} LUFS</text> }
        </g>
      }
    </svg>
  `,
  styles: `
    :host { display: block; width: 100%; }
    svg { display: block; }
    .band { fill: transparent; }
    text, .grid, .area, .trace, .ref, .target, .cursor, .tip { pointer-events: none; }
    .band.loud { fill: rgba(233, 196, 106, 0.07); }
    .band.quiet { fill: rgba(255, 255, 255, 0.012); }
    .band.silence { fill: rgba(232, 102, 95, 0.16); }
    .sec.clickable { cursor: pointer; }
    .sec.clickable:hover .band { stroke: var(--line); }
    .sec.selected .band { stroke: var(--a); stroke-width: 1.5; }
    .sec-label { fill: var(--muted); font-size: 11px; }
    .sec-value { fill: var(--ink); font-size: 12px; font-weight: 600; }
    .grid { stroke: var(--line-soft); stroke-width: 1; }
    .axis { fill: var(--faint); font-size: 11px; }
    .area.a { fill: rgba(233, 196, 106, 0.22); }
    .area.b { fill: rgba(124, 200, 216, 0.12); }
    .trace { fill: none; stroke-width: 1.6; stroke-linejoin: round; }
    .trace.a { stroke: var(--a); }
    .trace.b { stroke: var(--b); }
    .ref { stroke: var(--ink); stroke-dasharray: 5 4; opacity: 0.55; }
    .ref-label { fill: var(--ink); font-size: 11px; opacity: 0.8; }
    .target { stroke: var(--ok); stroke-dasharray: 2 4; opacity: 0.8; }
    .target-label { fill: var(--ok); font-size: 11px; }
    .cursor { stroke: var(--ink); opacity: 0.4; }
    .tip { fill: var(--raised); stroke: var(--line); }
    .tip-t { fill: var(--muted); font-size: 11px; }
    .tip-v { font-size: 12px; font-weight: 600; }
    .tip-v.a { fill: var(--a); }
    .tip-v.b { fill: var(--b); }
  `,
})
export class LoudnessBridge implements OnInit, OnDestroy {
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

  readonly H = 214;
  readonly top = 36;
  readonly plotH = 158;
  readonly left = 34;
  readonly num = num;

  private readonly el = inject(ElementRef<HTMLElement>);
  private ro?: ResizeObserver;
  readonly w = signal(900);
  readonly hover = signal<{ x: number; time: string; a: string; b: string | null } | null>(null);

  ngOnInit() {
    this.ro = new ResizeObserver((e) => this.w.set(Math.max(300, Math.floor(e[0].contentRect.width))));
    this.ro.observe(this.el.nativeElement);
  }
  ngOnDestroy() {
    this.ro?.disconnect();
  }

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
    })),
  );

  readonly paths = computed(() => {
    const series: Series[] = [];
    const cmp = this.compareValues();
    if (cmp) series.push({ values: cmp, hop: this.hop(), color: 'b' });
    series.push({ values: this.values(), hop: this.hop(), color: 'a' });
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
    const fmt = (v: number | undefined) => (v === undefined || v <= -100 ? 'silent' : num(v));
    this.hover.set({ x: px, time: clock(t), a: fmt(vals[i]), b: cmp ? fmt(cmp[i]) : null });
  }

  pick(i: number) {
    if (this.clickable() && this.sections()[i]?.kind !== 'silence') this.sectionClick.emit(i);
  }
}
