import { Component, computed, inject } from '@angular/core';
import { GenreMatch, KeyRole } from './api';
import { BandChart } from './band-chart';
import { hz, num, signed } from './fmt';
import { Readings, Reading } from './readings';
import { Store } from './store';

const ROLE_LABELS: Record<KeyRole, string> = {
  root: 'root', fifth: 'fifth', third: 'third', scale_tone: 'in key', out_of_key: 'out of key', between_notes: 'between notes',
};

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
          @if (genre(); as g) {
            <p class="muted lead">Against a typical {{ g.preset_name }} balance. Targets are rough starting points; a reference track is a better guide.</p>
          }
          <table class="data">
            <thead><tr><th>Band</th><th class="n">Share of total</th>
              @if (genre()) { <th class="n">Typical</th><th class="n">Difference</th> }
              <th></th></tr></thead>
            <tbody>
              @for (o of octaves(); track o.lo) {
                <tr>
                  <td>{{ hz(o.lo) }}–{{ hz(o.hi) }} Hz</td>
                  <td class="n">{{ num(o.share_db) }} dB</td>
                  @if (genre()) {
                    @if (o.band; as b) {
                      <td class="n faint">{{ num(b.target_db) }} dB</td>
                      <td class="n" [class.sev-warn]="abs(b.delta_db) > b.tolerance_db" [attr.title]="'Tolerance ±' + num(b.tolerance_db) + ' dB'">{{ signed(b.delta_db) }} dB</td>
                    } @else { <td></td><td></td> }
                  }
                  <td class="meter"><span [style.width.%]="barPct(o.share_db)"></span></td>
                </tr>
              }
            </tbody>
          </table>

          @if (genre(); as g) {
            <h2 class="sub">Against typical {{ g.preset_name }}</h2>
            <table class="data">
              <thead><tr><th>Measurement</th><th class="n">Value</th><th class="n">Typical</th></tr></thead>
              <tbody>
                @for (c of g.checks; track c.metric) {
                  <tr>
                    <td>{{ c.label }}</td>
                    <td class="n" [class.sev-info]="!c.within">{{ num(c.value, c.unit ? 1 : 2) }} <span class="faint">{{ c.unit }}</span></td>
                    <td class="n faint">{{ range(c.lo, c.hi, c.unit ? 1 : 2) }}</td>
                  </tr>
                }
              </tbody>
            </table>
          }
        </section>
        <section>
          <h2>Low-frequency peaks</h2>
          @if (key(); as k) {
            <p class="muted lead">Narrow peaks between 30 and 300 Hz, read in {{ k.key_name }}. Peaks on notes of the key are the bassline or chords; peaks out of key or between notes are likely resonances worth a narrow cut.</p>
            <table class="data">
              <thead><tr><th>Frequency</th><th>Note</th><th>In {{ k.key_name }}</th><th class="n">Cents</th><th class="n">Above median</th></tr></thead>
              <tbody>
                @for (p of k.peaks; track p.hz) {
                  <tr>
                    <td>{{ num(p.hz) }} Hz</td>
                    <td>{{ p.note }}</td>
                    <td [class.sev-warn]="offKey(p.role) && p.above_median_db >= 8" [class.muted]="offKey(p.role) && p.above_median_db < 8">{{ roleLabel(p.role) }}</td>
                    <td class="n faint">{{ signed(p.cents, 0) }}</td>
                    <td class="n">{{ signed(p.above_median_db) }} dB</td>
                  </tr>
                } @empty {
                  <tr><td colspan="5" class="muted">No prominent peaks.</td></tr>
                }
              </tbody>
            </table>

            <h2 class="sub">Bass notes in {{ k.key_name }}</h2>
            <p class="muted lead">Root and fifth frequencies{{ tuned() ? ', adjusted for the song’s tuning' : '' }}. Boosts here reinforce the key; cut candidates are the out-of-key peaks above.</p>
            <ul class="chips">
              @for (f of k.safe_freqs; track f.hz) {
                <li [class.root]="f.role === 'root'"><strong>{{ f.note }}</strong> {{ num(f.hz) }} Hz <span class="faint">{{ f.role }}</span></li>
              }
            </ul>
          } @else {
            <p class="muted lead">Narrow peaks between 30 and 300 Hz. Set the song's key above to tell bass notes from resonances.</p>
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
          }
        </section>
      </div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: 22px; }
    h2 { margin-bottom: 4px; }
    h2.sub { margin-top: 22px; }
    .lead { font-size: 12px; margin-bottom: 10px; max-width: 75ch; }
    .cols { display: grid; grid-template-columns: 1fr 1fr; gap: 32px; }
    .meter { width: 30%; }
    .meter span { display: block; height: 6px; background: var(--a); opacity: 0.7; border-radius: 3px; }
    .chips { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: 6px; }
    .chips li { border: 1px solid var(--line); border-radius: var(--radius-s); padding: 3px 8px; font-size: 12px; }
    .chips li.root { border-color: var(--a); }
    @media (max-width: 1100px) { .cols { grid-template-columns: 1fr; } }
  `,
})
export class Spectrum {
  private readonly store = inject(Store);
  readonly r = computed(() => this.store.report()?.report ?? null);
  readonly genre = computed<GenreMatch | null>(() => this.r()?.genre_match ?? null);
  readonly key = computed(() => this.r()?.key_analysis ?? null);
  readonly tuned = computed(() => Math.abs(this.key()?.tuning_offset_cents ?? 0) > 15);
  readonly hz = hz;
  readonly num = num;
  readonly signed = signed;
  readonly abs = Math.abs;

  readonly octaves = computed(() => {
    const r = this.r();
    if (!r) return [];
    const bands = this.genre()?.bands ?? [];
    return r.spectrum.octave_share.map((o) => ({ ...o, band: bands.find((b) => b.lo === o.lo) ?? null }));
  });

  barPct(db: number) {
    return Math.max(2, Math.min(100, ((db + 40) / 40) * 100));
  }

  range(lo: number | null, hi: number | null, digits: number) {
    if (lo != null && hi != null) return `${num(lo, digits)} to ${num(hi, digits)}`;
    if (lo != null) return `at least ${num(lo, digits)}`;
    if (hi != null) return `at most ${num(hi, digits)}`;
    return '';
  }

  roleLabel(r: KeyRole) {
    return ROLE_LABELS[r];
  }

  offKey(r: KeyRole) {
    return r === 'out_of_key' || r === 'between_notes';
  }

  readonly readings = computed<Reading[]>(() => {
    const r = this.r();
    if (!r) return [];
    const sp = r.spectrum;
    const sib = sp.sibilance;
    const g = this.genre();
    const tiltCheck = g?.checks.find((c) => c.metric === 'tilt');
    const list: Reading[] = [
      {
        label: 'Tilt', value: num(sp.tilt_db_per_oct), unit: 'dB/oct',
        note: tiltCheck ? `${g!.preset_name} ${this.range(tiltCheck.lo, tiltCheck.hi, 1)}` : 'pop mixes ≈ −2',
        severity: tiltCheck && !tiltCheck.within ? 'info' : null,
      },
      { label: '5–10 kHz, loudest 1%', value: num(sib.p99_db), unit: 'dB', note: 'vs full band', severity: r.findings.some((f) => f.code === 'sibilance') ? 'warn' : null },
      { label: '5–10 kHz, typical', value: num(sib.median_db), unit: 'dB' },
      { label: 'Below 30 Hz', value: num(sp.sub30_share_db), unit: 'dB', note: 'share of total', severity: r.findings.some((f) => f.code === 'subsonic') ? 'warn' : null },
    ];
    const k = this.key();
    if (k) {
      const c = k.tuning_offset_cents;
      list.push({
        label: 'Tuning', value: c == null ? '–' : signed(c, 0), unit: c == null ? undefined : 'cents',
        note: c == null ? 'too few peaks to tell' : `A4 ≈ ${num(k.a4_hz)} Hz`,
        severity: r.findings.some((f) => f.code === 'tuning') ? 'info' : null,
      });
    }
    return list;
  });
}
