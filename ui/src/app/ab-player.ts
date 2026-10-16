import { Component, ElementRef, computed, inject, input } from '@angular/core';
import { clock, num } from './fmt';
import { Player, Side } from './player';

/** Typing in these keeps its keys; everything else hears Space, 1/A and 2/B. */
const TEXT_ENTRY = 'input:not([type=range]):not([type=checkbox]), select, textarea, [contenteditable]';
/** Space already presses these, outside the player. */
const PRESSABLE = 'button, [role=button], [role=tab], a, input';

/** Each side's short-term loudness, for the contour above the position bar (reference mode). */
export interface Contour { current: number[]; partner: number[]; hop: number }

/** Contour drawing size in viewBox units; it stretches to the bar's width (keep CH in step with .contour's height). */
const CW = 1000;
const CH = 18;

/** Play, switch and scrub the pair in the Compare tab, level-matched by default. A is the open version (gold), B what it's compared with (cyan). */
@Component({
  selector: 'ml-ab-player',
  host: { '(document:keydown)': 'key($event)' },
  template: `
    <div class="bar" role="group" aria-label="Listen">
      <div class="grp">
        <button class="play" (click)="player.toggle()" [disabled]="!player.pair() || player.loading()">
          {{ player.loading() ? 'Loading…' : player.playing() ? 'Pause' : 'Play' }}
        </button>
        <div class="seg" role="group" aria-label="Hear">
          <button class="side-a" [class.on]="player.side() === 'current'" [attr.aria-pressed]="player.side() === 'current'"
            [attr.aria-label]="'A: ' + currentLabel()" [title]="currentLabel()" (click)="player.switchTo('current')">
            <i class="sw a"></i>A<kbd aria-hidden="true">1</kbd>
          </button>
          <button class="side-b" [class.on]="player.side() === 'partner'" [attr.aria-pressed]="player.side() === 'partner'"
            [attr.aria-label]="'B: ' + partnerLabel()" [title]="partnerLabel()" (click)="player.switchTo('partner')">
            <i class="sw b"></i>B<kbd aria-hidden="true">2</kbd>
          </button>
        </div>
      </div>
      <div class="grp pos" [class.contoured]="!!contourPath()">
        <span class="t">{{ clock(player.position()) }}</span>
        <div class="scrub-wrap">
          @if (contourPath(); as d) {
            <!-- The audible track's loudness, so there's something to aim at; clicking it seeks like the bar. -->
            <svg class="contour" [attr.viewBox]="'0 0 ' + CW + ' ' + CH" preserveAspectRatio="none" aria-hidden="true" (click)="seekContour($event)">
              <path [attr.d]="d" [attr.class]="player.side() === 'current' ? 'a' : 'b'" />
            </svg>
          }
          <input type="range" class="scrub" aria-label="Position" min="0" step="0.1" [max]="player.duration()" [value]="player.position()"
            [attr.aria-valuetext]="clock(player.position())" [style.--fill]="fill()" (input)="scrub($event)" />
        </div>
        <span class="t faint">{{ clock(player.duration()) }}</span>
      </div>
      <div class="grp">
        <label class="match">
          <input type="checkbox" [checked]="player.levelMatch()" [disabled]="!player.matchable()" (change)="match($event)" />
          Match loudness
        </label>
        <span class="gain muted">{{ gainText() }}</span>
      </div>
    </div>
    <p class="key">
      <span class="name" [title]="currentLabel()"><i class="sw a"></i><b>A</b> {{ currentLabel() }}</span>
      <span class="name" [title]="partnerLabel()"><i class="sw b"></i><b>B</b> {{ partnerLabel() }}</span>
      <span class="note" [class.err]="!!player.error()" role="status">{{ note() }}</span>
    </p>
  `,
  styles: `
    :host { display: block; --thumb: 12px; --ease: cubic-bezier(0.25, 1, 0.5, 1); }
    /* Tight inside a group (transport, position, level), wider between groups. */
    .bar { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-xs) var(--space-m); }
    .grp { display: flex; align-items: center; gap: var(--space-xs); }
    .pos { flex: 1 1 240px; min-width: 200px; }
    /* With a contour the bar sits low, so the clock reads level with the bar rather than the drawing. */
    .pos.contoured { align-items: flex-end; }
    /* Neutral: gold means "this version" here, and is kept for the page's one primary action. */
    /* Wide enough for 'Loading…', so the bar doesn't shift as the label changes. */
    .play { min-width: 92px; font-weight: 600; border-color: var(--line-strong); }
    .seg button { display: inline-flex; align-items: center;
      transition: color 120ms var(--ease), background-color 120ms var(--ease), box-shadow 120ms var(--ease); }
    /* The selection is underlined in its own colour, so the audible side reads at a glance. */
    .seg button.on.side-a { box-shadow: inset 0 -2px 0 var(--a); }
    .seg button.on.side-b { box-shadow: inset 0 -2px 0 var(--b); }
    kbd { font: inherit; font-size: calc(var(--text-caption) - 2px); font-weight: 400; line-height: 1; padding: 2px 4px;
      margin-left: 6px; border: 1px solid var(--line); border-radius: 3px; color: var(--faint); }
    /* --faint is too dim on --raised, the selected button's background. */
    .seg button.on kbd { color: var(--muted); }
    .sw { display: inline-block; flex: none; width: 10px; height: 10px; border-radius: 2px; margin-right: 6px; }
    .sw.a { background: var(--a); }
    .sw.b { background: var(--b); }
    .t { font-variant-numeric: tabular-nums; font-size: var(--text-caption); min-width: 4ch; text-align: center; line-height: 20px; }
    .scrub-wrap { flex: 1; min-width: 120px; display: flex; flex-direction: column; }
    /* Inset half a thumb each side: the thumb's centre, and so the playhead, travels that span. */
    .contour { display: block; width: calc(100% - var(--thumb)); height: 18px; margin: 0 calc(var(--thumb) / 2); cursor: pointer; }
    .contour path { transition: fill 120ms var(--ease); }
    .contour .a { fill: color-mix(in srgb, var(--a) 28%, transparent); }
    .contour .b { fill: color-mix(in srgb, var(--b) 28%, transparent); }
    /* Drawn here rather than natively: the native thumb stops short of both ends of its track. */
    .scrub { width: 100%; height: 20px; margin: 0; padding: 0; appearance: none; background: transparent; cursor: pointer; }
    .scrub::-webkit-slider-runnable-track { height: 4px; border-radius: 2px;
      /* The played part ends under the thumb's centre, which travels from half a thumb in at each end. */
      background: linear-gradient(to right, var(--ink) calc(var(--thumb) / 2 + var(--fill, 0) * (100% - var(--thumb))),
        var(--line-strong) 0); }
    .scrub::-webkit-slider-thumb { appearance: none; width: var(--thumb); height: var(--thumb); margin-top: -4px;
      border-radius: 50%; background: var(--ink); border: none; }
    .scrub:focus { outline: none; }
    .scrub:focus-visible::-webkit-slider-thumb { box-shadow: 0 0 0 2px var(--bg), 0 0 0 4px var(--focus); }
    .match { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
    .match input { accent-color: var(--ink); }
    .gain { font-size: var(--text-caption); font-variant-numeric: tabular-nums; }
    /* The colour key for the chart below, and which way the pair plays. */
    .key { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--space-2xs) var(--space-m); margin-top: var(--space-2xs);
      font-size: var(--text-caption); color: var(--muted); min-height: 1.4em; }
    .name { display: inline-block; max-width: 32ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .name b { color: var(--ink); margin-right: 2px; }
    .name .sw { width: 8px; height: 8px; vertical-align: 0; }
    .note { color: var(--faint); }
    .note.err { color: var(--err); }
    @media (prefers-reduced-motion: reduce) { .seg button, .contour path { transition: none; } }
  `,
})
export class AbPlayer {
  readonly player = inject(Player);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  readonly currentLabel = input('');
  readonly partnerLabel = input('');
  readonly contour = input<Contour | null>(null);
  readonly clock = clock;
  readonly CW = CW;
  readonly CH = CH;

  /** Share of the bar played, 0 to 1. */
  readonly fill = computed(() => {
    const d = this.player.duration();
    return d > 0 ? Math.min(1, this.player.position() / d) : 0;
  });

  /** Area path of the audible side's short-term loudness over its own length, the top 30 dB of it. */
  readonly contourPath = computed(() => {
    const c = this.contour();
    const dur = this.player.duration();
    const vals = c?.[this.player.side()];
    if (!c || !vals?.length || dur <= 0) return null;
    const heard = vals.filter((v) => v > -100);
    if (!heard.length) return null;
    const top = Math.max(...heard);
    const floor = top - 30;
    // Keep the loudest value in each bucket, so short peaks survive the thinning.
    const step = Math.max(1, Math.ceil(vals.length / 500));
    const pts: string[] = [];
    for (let i = 0; i < vals.length; i += step) {
      const v = Math.max(...vals.slice(i, i + step));
      // Each value covers the 3 s ending at (i+1)·hop, as in the loudness timeline.
      const t = Math.max(0, (i + step / 2 + 0.5) * c.hop - 1.5);
      const x = Math.min(CW, (t / dur) * CW);
      const y = CH - (Math.max(0, Math.min(30, v - floor)) / 30) * (CH - 1);
      pts.push(`${x.toFixed(1)},${y.toFixed(1)}`);
    }
    return `M${pts[0].split(',')[0]},${CH}L${pts.join('L')}L${pts[pts.length - 1].split(',')[0]},${CH}Z`;
  });

  readonly gainText = computed(() => {
    const p = this.player.pair();
    if (!p) return '';
    if (!this.player.matchable()) return "Can't match: one file has no measurable loudness";
    if (!this.player.levelMatch()) return 'Original levels';
    const d = p.loudnessDiff;
    if (Math.abs(d) < 0.05) return 'Already the same loudness';
    const louder: Side = d > 0 ? 'partner' : 'current';
    return `${louder === 'partner' ? 'B' : 'A'} turned down ${num(Math.abs(d))} dB`;
  });

  readonly note = computed(() => {
    const e = this.player.error();
    if (e) return e;
    const p = this.player.pair();
    if (!p) return '';
    if (this.player.loading()) return 'Loading audio…';
    return p.mode === 'reference' ? 'Each keeps its own position' : 'In sync: you hear the same moment';
  });

  scrub(ev: Event) {
    this.player.seek(+(ev.target as HTMLInputElement).value);
  }

  seekContour(ev: MouseEvent) {
    const r = (ev.currentTarget as SVGElement).getBoundingClientRect();
    this.player.seek(Math.max(0, Math.min(1, (ev.clientX - r.left) / r.width)) * this.player.duration());
  }

  match(ev: Event) {
    this.player.setLevelMatch((ev.target as HTMLInputElement).checked);
  }

  key(ev: KeyboardEvent) {
    if (ev.ctrlKey || ev.metaKey || ev.altKey || ev.repeat || !this.player.pair()) return;
    const t = ev.target instanceof Element ? ev.target : null;
    if (t?.closest(TEXT_ENTRY) || t?.closest('dialog')) return;
    const k = ev.key.toLowerCase();
    if (k === ' ') {
      // Space on the player's own controls still means play/pause: pressing a focused A or B again,
      // or ticking Match loudness mid-comparison, is never what's meant.
      if (t?.closest(PRESSABLE) && !this.host.nativeElement.contains(t)) return;
      ev.preventDefault();
      void this.player.toggle();
    } else if (k === '1' || k === 'a') {
      this.player.switchTo('current');
    } else if (k === '2' || k === 'b') {
      this.player.switchTo('partner');
    }
  }
}
