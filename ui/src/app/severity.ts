import { Component, computed, input } from '@angular/core';
import { Severity } from './api';

const SPOKEN: Record<Severity, string> = { error: 'Problem', warn: 'Check', info: 'Note', ok: 'Passed' };

/**
 * Severity marker. Each level has its own shape as well as its own colour, so it reads
 * without colour vision: diamond = problem, triangle = check, ring = note, tick = passed.
 */
@Component({
  selector: 'ml-sev',
  host: { '[attr.data-sev]': 'sev()' },
  template: `
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      @switch (sev()) {
        @case ('error') { <path class="solid" d="M5 0.4 9.6 5 5 9.6 0.4 5Z" /> }
        @case ('warn') { <path class="solid" d="M5 0.6 9.7 9.3H0.3Z" /> }
        @case ('info') { <circle class="line" cx="5" cy="5" r="3.7" /> }
        @default { <path class="line" d="M1.2 5.3 3.9 8 8.8 2.2" /> }
      }
    </svg>
    @if (announce()) { <span class="sr-only">{{ spoken() }}: </span> }
  `,
  styles: `
    :host { display: inline-flex; flex: none; width: 10px; height: 10px; color: var(--muted); }
    :host([data-sev='error']) { color: var(--err); }
    :host([data-sev='warn']) { color: var(--warn); }
    :host([data-sev='info']) { color: var(--info); }
    :host([data-sev='ok']) { color: var(--ok); }
    svg { display: block; overflow: visible; }
    .solid { fill: currentColor; }
    .line { fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }
  `,
})
export class Sev {
  readonly sev = input.required<Severity>();
  /** False when nearby text already says the severity. */
  readonly announce = input(true);
  readonly spoken = computed(() => SPOKEN[this.sev()]);
}
