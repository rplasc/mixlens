import { Component, input } from '@angular/core';
import { Severity } from './api';
import { Sev } from './severity';

export interface Reading {
  label: string;
  value: string;
  unit?: string;
  note?: string;
  severity?: Severity | null;
}

/** A row of large measured values. `size` sets how prominent they are. */
@Component({
  selector: 'ml-readings',
  imports: [Sev],
  host: { '[class.large]': "size() === 'l'", '[class.small]': "size() === 's'" },
  template: `
    <dl>
      @for (r of items(); track r.label) {
        <div class="reading" [attr.data-sev]="r.severity ?? null">
          <dt>{{ r.label }}@if (r.severity) { <ml-sev [sev]="r.severity" /> }</dt>
          <dd>
            <span class="value">{{ r.value }}</span>
            @if (r.unit) { <span class="unit">{{ r.unit }}</span> }
          </dd>
          @if (r.note) { <dd class="note">{{ r.note }}</dd> }
        </div>
      }
    </dl>
  `,
  styles: `
    dl { display: grid; grid-template-columns: repeat(auto-fill, minmax(104px, 1fr)); gap: 0; margin: 0; }
    .reading { padding: 10px 10px 10px 14px; }
    dt { font-size: var(--text-caption); color: var(--muted); display: flex; align-items: center; gap: 5px; }
    dd { margin: 2px 0 0; }
    .value { font-family: var(--font-display); font-size: var(--num-m); font-weight: 600; line-height: 1.15; letter-spacing: -0.02em; }
    .unit { color: var(--muted); font-size: var(--text-caption); margin-left: var(--space-2xs); }
    .note { font-size: var(--text-caption); color: var(--faint); text-wrap: balance; }
    :host(.large) dl { grid-template-columns: repeat(auto-fit, minmax(124px, 1fr)); }
    :host(.large) .value { font-size: var(--num-l); }
    :host(.small) dl { grid-template-columns: repeat(auto-fit, minmax(100px, 1fr)); }
    :host(.small) .reading { padding-right: 6px; }
    :host(.small) .value { font-size: var(--num-s); }
  `,
})
export class Readings {
  readonly items = input.required<Reading[]>();
  readonly size = input<'s' | 'm' | 'l'>('m');
}
