import { Component, ElementRef, OnDestroy, OnInit, computed, inject, input, signal } from '@angular/core';
import { BandDev } from './api';
import { hz, signed } from './fmt';

/** Third-octave deviation from the fitted tilt: bumps up, dips down. Optional B overlay. */
@Component({
  selector: 'ml-band-chart',
  template: `
    <svg [attr.width]="w()" [attr.height]="height()" role="img" aria-label="Tonal balance relative to the overall tilt">
      @for (g of grid(); track g.db) {
        <line [attr.x1]="left" [attr.x2]="w()" [attr.y1]="g.y" [attr.y2]="g.y" [class.zero]="g.db === 0" class="grid" />
        <text [attr.x]="left - 6" [attr.y]="g.y + 4" class="axis" text-anchor="end">{{ g.label }}</text>
      }
      @for (b of bars(); track b.fc) {
        <g>
          <title>{{ b.title }}</title>
          <rect [attr.x]="b.x" [attr.y]="b.ya" [attr.width]="b.bw" [attr.height]="b.ha" [class.flag]="b.flag" class="bar a" />
          @if (b.yb !== null) {
            <rect [attr.x]="b.x + b.bw + 1" [attr.y]="b.yb" [attr.width]="b.bw" [attr.height]="b.hb" class="bar b" />
          }
        </g>
      }
      @for (l of labels(); track l.text) {
        <text [attr.x]="l.x" [attr.y]="height() - 4" class="axis" text-anchor="middle">{{ l.text }}</text>
      }
    </svg>
  `,
  styles: `
    :host { display: block; width: 100%; }
    svg { display: block; }
    .grid { stroke: var(--line-soft); }
    .grid.zero { stroke: var(--muted); opacity: 0.6; }
    .axis { fill: var(--faint); font-size: var(--text-caption); }
    .bar.a { fill: var(--a); opacity: 0.85; }
    .bar.a.flag { fill: var(--warn); opacity: 1; }
    .bar.b { fill: var(--b); opacity: 0.85; }
  `,
})
export class BandChart implements OnInit, OnDestroy {
  readonly a = input.required<BandDev[]>();
  readonly b = input<BandDev[] | null>(null);
  readonly height = input(200);
  /** Highlight deviations larger than this in the 150 Hz–6 kHz region (A only). */
  readonly flagAbove = input(3.5);

  readonly left = 34;
  private readonly el = inject(ElementRef<HTMLElement>);
  private ro?: ResizeObserver;
  readonly w = signal(700);

  ngOnInit() {
    this.ro = new ResizeObserver((e) => this.w.set(Math.max(280, Math.floor(e[0].contentRect.width))));
    this.ro.observe(this.el.nativeElement);
  }
  ngOnDestroy() {
    this.ro?.disconnect();
  }

  private readonly bands = computed(() => this.a().filter((x) => x.fc >= 30));
  private readonly lim = 9;
  private plotH() {
    return this.height() - 22;
  }
  y(db: number) {
    const c = Math.max(-this.lim, Math.min(this.lim, db));
    return 6 + ((this.lim - c) / (2 * this.lim)) * (this.plotH() - 6);
  }

  readonly grid = computed(() => [6, 3, 0, -3, -6].map((db) => ({ db, y: this.y(db), label: db === 0 ? '0' : signed(db, 0) })));

  readonly bars = computed(() => {
    const list = this.bands();
    const slot = (this.w() - this.left) / list.length;
    const hasB = !!this.b();
    const bw = Math.max(2, (hasB ? slot * 0.36 : slot * 0.62));
    const y0 = this.y(0);
    return list.map((band, i) => {
      const x = this.left + i * slot + (slot - (hasB ? bw * 2 + 1 : bw)) / 2;
      const ya = this.y(Math.max(0, band.dev_db));
      const ha = Math.max(1, Math.abs(this.y(band.dev_db) - y0));
      const bb = this.b()?.find((x) => Math.abs(x.fc - band.fc) < 0.5) ?? null;
      const flag = !hasB && band.fc >= 150 && band.fc <= 6000 && Math.abs(band.dev_db) > this.flagAbove();
      return {
        fc: band.fc,
        x,
        bw,
        ya,
        ha,
        yb: bb ? this.y(Math.max(0, bb.dev_db)) : null,
        hb: bb ? Math.max(1, Math.abs(this.y(bb.dev_db) - y0)) : 0,
        flag,
        title: `${hz(band.fc)} Hz: ${signed(band.dev_db)} dB` + (bb ? ` → ${signed(bb.dev_db)} dB` : ''),
      };
    });
  });

  readonly labels = computed(() => {
    const list = this.bands();
    const slot = (this.w() - this.left) / list.length;
    const want = [63, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
    return want
      .map((f) => {
        const i = list.findIndex((b) => Math.abs(Math.log2(b.fc / f)) < 0.17);
        return i < 0 ? null : { x: this.left + i * slot + slot / 2, text: hz(f) };
      })
      .filter((x): x is { x: number; text: string } => !!x);
  });
}
