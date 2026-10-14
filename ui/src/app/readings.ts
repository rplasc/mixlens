import { Component, input } from '@angular/core';
import { Severity } from './api';

export interface Reading {
  label: string;
  value: string;
  unit?: string;
  note?: string;
  severity?: Severity | null;
}

/** A row of large measured values. */
@Component({
  selector: 'ml-readings',
  template: `
    <dl>
      @for (r of items(); track r.label) {
        <div class="reading" [attr.data-sev]="r.severity ?? null">
          <dt>{{ r.label }}</dt>
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
    .reading { padding: 10px 10px 10px 12px; border-left: 2px solid transparent; }
    .reading[data-sev='warn'] { border-left-color: var(--warn); }
    .reading[data-sev='error'] { border-left-color: var(--err); }
    dt { font-size: 12px; color: var(--muted); }
    dd { margin: 2px 0 0; }
    .value { font-family: var(--font-display); font-size: 24px; font-weight: 600; line-height: 1.15; letter-spacing: -0.02em; }
    .unit { color: var(--muted); font-size: 12px; margin-left: 4px; }
    .note { font-size: 12px; color: var(--faint); }
  `,
})
export class Readings {
  readonly items = input.required<Reading[]>();
}
