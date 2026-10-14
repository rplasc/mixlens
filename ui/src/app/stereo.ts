import { Component, computed, inject } from '@angular/core';
import { num, signed, time } from './fmt';
import { Reading, Readings } from './readings';
import { Store } from './store';

@Component({
  selector: 'ml-stereo',
  imports: [Readings],
  template: `
    @if (r(); as r) {
      <ml-readings [items]="readings()" />

      <section class="gauge-wrap">
        <h2>Correlation across the song</h2>
        <p class="muted lead">How correlation varies across 400 ms windows. +1 is mono, 0 is unrelated, and anything below 0 cancels in mono.</p>
        <div class="gauge" role="img" [attr.aria-label]="'Window correlation from ' + num(r.stereo.window_corr_min, 2) + ' to 1'">
          <div class="scale"><span>−1</span><span>0</span><span>+1</span></div>
          <div class="track">
            <div class="zero"></div>
            <div class="span" [style.left.%]="pos(r.stereo.window_corr_p5)" [style.width.%]="pos(1) - pos(r.stereo.window_corr_p5)"></div>
            <div class="mark min" [class.bad]="r.stereo.window_corr_min < 0" [style.left.%]="pos(r.stereo.window_corr_min)" title="Minimum"></div>
            <div class="mark med" [style.left.%]="pos(r.stereo.window_corr_median)" title="Median"></div>
          </div>
          <div class="legend muted">
            Minimum {{ num(r.stereo.window_corr_min, 2) }}, 5th percentile {{ num(r.stereo.window_corr_p5, 2) }}, median {{ num(r.stereo.window_corr_median, 2) }}
          </div>
        </div>
      </section>

      <section>
        <h2>Lowest-correlation moments</h2>
        <table class="data">
          <thead><tr><th>Time</th><th class="n">Correlation</th><th class="n">Level</th><th></th></tr></thead>
          <tbody>
            @for (w of r.stereo.low_corr_windows; track w.t) {
              <tr>
                <td>{{ time(w.t) }}</td>
                <td class="n" [class.sev-warn]="w.corr < 0 && w.level_dbfs > -40">{{ num(w.corr, 2) }}</td>
                <td class="n">{{ num(w.level_dbfs) }} dBFS</td>
                <td class="faint">{{ w.level_dbfs < -40 ? 'quiet, likely a reverb tail' : '' }}</td>
              </tr>
            } @empty {
              <tr><td colspan="4" class="muted">No moments below 0.35.</td></tr>
            }
          </tbody>
        </table>
      </section>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: 22px; }
    h2 { margin-bottom: var(--space-2xs); }
    .lead { font-size: var(--text-caption); margin-bottom: var(--space-s); }
    .gauge { max-width: 640px; }
    .scale { display: flex; justify-content: space-between; font-size: var(--text-caption); color: var(--faint); margin-bottom: var(--space-2xs); }
    .track { position: relative; height: 18px; background: var(--panel); border: 1px solid var(--line); border-radius: 9px; }
    .zero { position: absolute; left: 50%; top: 2px; bottom: 2px; width: 1px; background: var(--muted); }
    .span { position: absolute; top: 4px; bottom: 4px; background: color-mix(in srgb, var(--a) 35%, transparent); border-radius: 5px; }
    .mark { position: absolute; top: -3px; width: 2px; height: 22px; margin-left: -1px; }
    .mark.min { background: var(--muted); }
    .mark.min.bad { background: var(--warn); }
    .mark.med { background: var(--a); }
    .legend { font-size: var(--text-caption); margin-top: var(--space-xs); }
  `,
})
export class Stereo {
  private readonly store = inject(Store);
  readonly r = computed(() => this.store.report()?.report ?? null);
  readonly num = num;
  readonly time = time;

  pos(c: number) {
    return ((Math.max(-1, Math.min(1, c)) + 1) / 2) * 100;
  }

  readonly readings = computed<Reading[]>(() => {
    const r = this.r();
    if (!r) return [];
    const s = r.stereo;
    const sev = (code: string) => r.findings.find((f) => f.code === code)?.severity ?? null;
    return [
      { label: 'Correlation', value: num(s.correlation, 2) },
      { label: 'Side vs mid', value: num(s.side_mid_db), unit: 'dB' },
      { label: 'Below 120 Hz', value: num(s.low_correlation, 2), note: `side ${num(s.low_side_mid_db)} dB`, severity: sev('low_end_width') },
      { label: 'Mono fold', value: signed(s.mono_loss_lu, 2), unit: 'LU', note: `${num(s.mono_fold_lufs)} LUFS in mono`, severity: sev('mono') },
      { label: 'L/R balance', value: signed(r.loudness.lr_balance_db, 2), unit: 'dB', severity: sev('lr_balance') },
    ];
  });
}
