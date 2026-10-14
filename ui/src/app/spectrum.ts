import { Component, computed, inject } from '@angular/core';
import { BandChart } from './band-chart';
import { hz, num, signed } from './fmt';
import { Readings, Reading } from './readings';
import { Store } from './store';

@Component({
  selector: 'ml-spectrum',
  imports: [BandChart, Readings],
  template: `
    @if (r(); as r) {
      <ml-readings [items]="readings()" />
      <section>
        <h2>Balance against the overall tilt</h2>
        <p class="muted lead">Each bar is a third-octave band's distance from the song's fitted slope. Bumps above about 3.5 dB between 150 Hz and 6 kHz are highlighted.</p>
        <ml-band-chart [a]="r.spectrum.third_octave" [height]="260" />
      </section>

      <div class="cols">
        <section>
          <h2>Energy by octave</h2>
          <table class="data">
            <thead><tr><th>Band</th><th class="n">Share of total</th><th></th></tr></thead>
            <tbody>
              @for (o of r.spectrum.octave_share; track o.lo) {
                <tr>
                  <td>{{ hz(o.lo) }}–{{ hz(o.hi) }} Hz</td>
                  <td class="n">{{ num(o.share_db) }} dB</td>
                  <td class="meter"><span [style.width.%]="barPct(o.share_db)"></span></td>
                </tr>
              }
            </tbody>
          </table>
        </section>
        <section>
          <h2>Low-frequency peaks</h2>
          <p class="muted lead">Narrow peaks between 30 and 300 Hz. Peaks on note pitches are the bassline or chords, not resonances.</p>
          <table class="data">
            <thead><tr><th>Frequency</th><th>Note</th><th class="n">Above median</th></tr></thead>
            <tbody>
              @for (p of r.spectrum.lf_peaks; track p.hz) {
                <tr><td>{{ num(p.hz) }} Hz</td><td>{{ p.note }}</td><td class="n">{{ signed(p.above_median_db) }} dB</td></tr>
              } @empty {
                <tr><td colspan="3" class="muted">No prominent peaks.</td></tr>
              }
            </tbody>
          </table>
        </section>
      </div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: 22px; }
    h2 { margin-bottom: 4px; }
    .lead { font-size: 12px; margin-bottom: 10px; max-width: 75ch; }
    .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 32px; }
    .meter { width: 40%; }
    .meter span { display: block; height: 6px; background: var(--a); opacity: 0.7; border-radius: 3px; }
    @media (max-width: 1100px) { .cols { grid-template-columns: 1fr; } }
  `,
})
export class Spectrum {
  private readonly store = inject(Store);
  readonly r = computed(() => this.store.report()?.report ?? null);
  readonly hz = hz;
  readonly num = num;
  readonly signed = signed;

  barPct(db: number) {
    return Math.max(2, Math.min(100, ((db + 40) / 40) * 100));
  }

  readonly readings = computed<Reading[]>(() => {
    const r = this.r();
    if (!r) return [];
    const sp = r.spectrum;
    const sib = sp.sibilance;
    return [
      { label: 'Tilt', value: num(sp.tilt_db_per_oct), unit: 'dB/oct', note: 'pop mixes ≈ −2' },
      { label: '5–10 kHz, loudest 1%', value: num(sib.p99_db), unit: 'dB', note: 'vs full band', severity: r.findings.some((f) => f.code === 'sibilance') ? 'warn' : null },
      { label: '5–10 kHz, typical', value: num(sib.median_db), unit: 'dB' },
      { label: 'Below 30 Hz', value: num(sp.sub30_share_db), unit: 'dB', note: 'share of total', severity: r.findings.some((f) => f.code === 'subsonic') ? 'warn' : null },
    ];
  });
}
