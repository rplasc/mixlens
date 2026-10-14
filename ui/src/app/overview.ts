import { Component, computed, inject } from '@angular/core';
import { Finding, Report } from './api';
import { BandChart } from './band-chart';
import { format, num, signed, time } from './fmt';
import { LoudnessBridge } from './loudness-bridge';
import { Reading, Readings } from './readings';
import { Store } from './store';

const READING_CODES: Record<string, string[]> = {
  Integrated: ['loudness', 'loudness_low', 'loudness_high'],
  'True peak': ['tp_ceiling', 'mix_headroom', 'clipping', 'over_full_scale'],
  PLR: ['mix_overcompressed'],
  'Mono fold': ['mono'],
  'Low-end corr.': ['low_end_width'],
};

@Component({
  selector: 'ml-overview',
  imports: [LoudnessBridge, Readings, BandChart],
  template: `
    @if (r(); as r) {
      <ml-readings [items]="readings()" />

      <section class="bridge">
        <div class="bridge-head">
          <h2>Loudness over time</h2>
          <p class="muted">Short-term loudness, 3 s windows. Select a section to measure it.</p>
        </div>
        <ml-loudness-bridge
          [values]="r.loudness.short_term" [hop]="r.loudness.short_term_hop_s" [sections]="r.sections"
          [integrated]="r.loudness.integrated_lufs" [target]="isMaster() ? (store.settings()?.target_lufs ?? -14) : null"
          [duration]="r.file.duration_s" [clickable]="true" [selected]="store.sectionIndex()"
          (sectionClick)="store.inspectSection($event)" />
      </section>

      @if (store.section(); as s) {
        <section class="inspect">
          <div class="inspect-head">
            <h2>{{ time(s.loudness.start_s) }} to {{ time(s.loudness.end_s) }}</h2>
            <span><strong>{{ num(s.loudness.lufs) }} LUFS</strong>
              <span class="muted">, {{ signed(s.loudness.vs_integrated_lu) }} LU vs whole song, tilt {{ num(s.bands.tilt_db_per_oct) }} dB/oct</span></span>
            <button class="ghost" (click)="store.inspectSection(store.sectionIndex()!)">Close</button>
          </div>
          <ml-band-chart [a]="s.bands.third_octave" [height]="150" />
        </section>
      }

      <div class="cols">
        <section>
          <h2>Findings</h2>
          <ul class="findings">
            @for (f of r.findings; track f.code + f.detail) {
              <li [attr.data-sev]="f.severity">
                <span class="dot" aria-hidden="true"></span>
                <div>
                  <strong>{{ f.title }}</strong>
                  <p>{{ f.detail }}</p>
                </div>
              </li>
            }
          </ul>
        </section>

        <section>
          <h2>File</h2>
          <table class="data">
            <tbody>
              <tr><td class="muted">Format</td><td>{{ fmtFormat(r) }}</td></tr>
              <tr><td class="muted">Length</td><td>{{ time(r.file.duration_s) }}</td></tr>
              <tr><td class="muted">First sound</td><td>{{ time(r.edges.first_sound_s) }}</td></tr>
              <tr><td class="muted">Last sound</td><td>{{ time(r.edges.last_sound_s) }}, then {{ num(r.edges.trailing_silence_s) }} s
                {{ r.edges.ends_on_silence ? 'of silence' : '(still sounding at the end)' }}</td></tr>
              <tr><td class="muted">Fade-out</td><td>{{ r.edges.fade_out_detected ? 'Yes' : 'No' }}</td></tr>
              <tr><td class="muted">Sample peak</td><td>L {{ num(r.loudness.sample_peak_l_dbfs, 2) }}, R {{ num(r.loudness.sample_peak_r_dbfs, 2) }} dBFS</td></tr>
              <tr><td class="muted">Crest factor</td><td>{{ num(r.loudness.crest_db) }} dB</td></tr>
              @if (r.dither) {
                <tr><td class="muted">Dither</td><td>{{ r.dither.detected ? 'Present' : r.dither.samples_checked ? 'Not detected' : 'Not checked' }}
                  <span class="faint">({{ r.dither.bits }}-bit)</span></td></tr>
              }
              <tr><td class="muted">Analysis time</td><td>{{ (r.analysis_ms / 1000).toFixed(1) }} s</td></tr>
            </tbody>
          </table>

          <div class="codec">
            <div class="codec-head">
              <h2>Streaming encode check</h2>
              <button (click)="store.runCodec()" [disabled]="store.codecBusy()">
                {{ store.codecBusy() ? 'Encoding…' : store.codec() ? 'Run again' : 'Run check' }}
              </button>
            </div>
            <p class="muted">Encodes to Ogg and AAC with ffmpeg and measures the peaks listeners will actually get.</p>
            @if (store.codec(); as rows) {
              <table class="data">
                <thead><tr><th>Encode</th><th class="n">True peak</th><th class="n">Loudness</th></tr></thead>
                <tbody>
                  @for (c of rows; track c.codec) {
                    <tr>
                      <td>{{ c.label }}</td>
                      @if (c.error) { <td colspan="2" class="sev-error">{{ c.error }}</td> }
                      @else {
                        <td class="n" [class.sev-error]="(c.true_peak_dbtp ?? -99) > 0" [class.sev-warn]="(c.true_peak_dbtp ?? -99) > -0.5 && (c.true_peak_dbtp ?? -99) <= 0">
                          {{ num(c.true_peak_dbtp) }} dBTP</td>
                        <td class="n">{{ num(c.integrated_lufs) }} LUFS</td>
                      }
                    </tr>
                  }
                </tbody>
              </table>
            }
          </div>
        </section>
      </div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: 22px; }
    section h2 { margin-bottom: 8px; }
    .bridge-head { display: flex; align-items: baseline; gap: 12px; margin-bottom: 6px; }
    .bridge-head p { font-size: 12px; }
    .inspect { border: 1px solid var(--line); border-radius: var(--radius-m); padding: 12px 14px; background: var(--panel); }
    .inspect-head { display: flex; align-items: baseline; gap: 14px; margin-bottom: 6px; }
    .inspect-head button { margin-left: auto; }
    .cols { display: grid; grid-template-columns: minmax(0, 1.25fr) minmax(0, 1fr); gap: 32px; }
    .findings { list-style: none; margin: 0; padding: 0; }
    .findings li { display: grid; grid-template-columns: 14px 1fr; gap: 8px; padding: 8px 0; border-bottom: 1px solid var(--line-soft); }
    .findings p { color: var(--muted); max-width: 70ch; }
    .dot { width: 8px; height: 8px; border-radius: 50%; margin-top: 6px; background: var(--muted); }
    li[data-sev='error'] .dot { background: var(--err); }
    li[data-sev='warn'] .dot { background: var(--warn); }
    li[data-sev='ok'] .dot { background: var(--ok); }
    li[data-sev='info'] .dot { background: var(--info); }
    .codec { margin-top: 22px; }
    .codec-head { display: flex; align-items: center; justify-content: space-between; }
    .codec-head h2 { margin: 0; }
    .codec p { font-size: 12px; margin: 4px 0 8px; }
    @media (max-width: 1100px) { .cols { grid-template-columns: 1fr; } }
  `,
})
export class Overview {
  readonly store = inject(Store);
  readonly num = num;
  readonly signed = signed;
  readonly time = time;
  readonly r = computed(() => this.store.report()?.report ?? null);
  readonly isMaster = computed(() => this.store.report()?.kind === 'master');

  fmtFormat(r: Report) {
    return format(r.file.sample_format, r.file.sample_rate);
  }

  private sevFor(label: string, findings: Finding[]) {
    const codes = READING_CODES[label] ?? [];
    const hit = findings.filter((f) => codes.includes(f.code));
    if (!hit.length) return null;
    const order = ['ok', 'info', 'warn', 'error'];
    return hit.map((f) => f.severity).sort((a, b) => order.indexOf(b) - order.indexOf(a))[0];
  }

  readonly readings = computed<Reading[]>(() => {
    const r = this.r();
    if (!r) return [];
    const l = r.loudness;
    const s = r.stereo;
    const f = r.findings;
    const target = this.store.settings()?.target_lufs ?? -14;
    return [
      { label: 'Integrated', value: num(l.integrated_lufs), unit: 'LUFS', note: this.isMaster() ? `target ${num(target, 0)}` : undefined, severity: this.sevFor('Integrated', f) },
      { label: 'True peak', value: num(l.true_peak_dbtp), unit: 'dBTP', severity: this.sevFor('True peak', f) },
      { label: 'Loudness range', value: num(l.lra_lu), unit: 'LU' },
      { label: 'PLR', value: num(l.plr_db), unit: 'dB', note: 'peak to loudness', severity: this.sevFor('PLR', f) },
      { label: 'Mono fold', value: signed(s.mono_loss_lu, 2), unit: 'LU', severity: this.sevFor('Mono fold', f) },
      { label: 'Correlation', value: num(s.correlation, 2), note: `below 120 Hz ${num(s.low_correlation, 2)}`, severity: this.sevFor('Low-end corr.', f) },
    ];
  });
}
