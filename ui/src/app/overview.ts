import { Component, computed, inject } from '@angular/core';
import { Finding } from './api';
import { BandChart } from './band-chart';
import { num, prose, signed, time } from './fmt';
import { LoudnessBridge } from './loudness-bridge';
import { Sev } from './severity';
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
  imports: [LoudnessBridge, Readings, BandChart, Sev],
  template: `
    @if (r(); as r) {
      <div class="summary">
        @if (verdict(); as v) {
          <header class="verdict" [attr.data-sev]="v.sev">
            <h2><ml-sev [sev]="v.sev" [announce]="false" />{{ v.headline }}</h2>
            @if (v.sub) { <p class="muted">{{ v.sub }}</p> }
          </header>
        }
        @if (kindTip(); as tip) {
          <div class="tip" role="note">
            <p>{{ tip }}</p>
            @if (isMaster()) { <button class="link" (click)="store.showSettings.set(true)">Change platform</button> }
            <button class="ghost" (click)="store.dismissKindTip()">Got it</button>
          </div>
        }
        <div class="measures">
          <section class="group delivery">
            <h3>Delivery</h3>
            <ml-readings [items]="delivery()" size="l" />
          </section>
          <section class="group character">
            <h3>Dynamics and stereo</h3>
            <ml-readings [items]="character()" size="s" />
          </section>
        </div>
      </div>

      <section class="bridge">
        <div class="bridge-head">
          <h2>Loudness over time</h2>
          <p class="muted">Short-term loudness in 3 s windows. Click a section to measure it on its own.</p>
        </div>
        <ml-loudness-bridge
          [values]="r.loudness.short_term" [hop]="r.loudness.short_term_hop_s" [sections]="r.sections"
          [integrated]="r.loudness.integrated_lufs" [target]="isMaster() ? target() : null"
          [duration]="r.file.duration_s" [clickable]="true" [selected]="store.sectionIndex()"
          (sectionClick)="store.inspectSection($event)" />
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
      </section>

      <div class="cols">
        <section>
          <h2>Findings</h2>
          <ul class="findings">
            @for (f of issues(); track f.code + f.detail) {
              <li>
                <ml-sev [sev]="f.severity" />
                <div>
                  <strong>{{ prose(f.title) }}</strong>
                  <p>{{ prose(f.detail) }}</p>
                  @if (f.code === 'genre_tone' && f.severity !== 'ok' && store.references()[0]; as ref) {
                    <button class="link" (click)="store.compareWithReference(ref.id)">Compare with {{ ref.label }}</button>
                  }
                </div>
              </li>
            }
          </ul>
          @if (passed().length) {
            <p class="passed"><span class="sev-ok">Passed:</span> {{ passed().join(' · ') }}</p>
          }
        </section>

        <section>
          <h2>File</h2>
          <table class="data">
            <tbody>
              <tr><td class="muted">First sound</td><td>{{ time(r.edges.first_sound_s) }}</td></tr>
              <tr><td class="muted">Last sound</td><td>{{ time(r.edges.last_sound_s) }}{{ r.edges.ends_on_silence
                ? ', then ' + num(r.edges.trailing_silence_s) + ' s of silence' : ', still sounding when the file ends' }}</td></tr>
              <tr><td class="muted">Fade-out</td><td>{{ r.edges.fade_out_detected ? 'Yes' : 'No' }}</td></tr>
              <tr><td class="muted">Sample peak</td><td>L {{ num(r.loudness.sample_peak_l_dbfs, 2) }}, R {{ num(r.loudness.sample_peak_r_dbfs, 2) }} dBFS</td></tr>
              <tr><td class="muted">Crest factor</td><td>{{ num(r.loudness.crest_db) }} dB</td></tr>
              @if (r.dither) {
                <tr><td class="muted">Dither</td><td>{{ r.dither.detected ? 'Present' : r.dither.samples_checked ? 'Not detected' : 'Not checked' }}
                  <span class="faint">({{ r.dither.bits }}-bit)</span></td></tr>
              }
            </tbody>
          </table>

        </section>
      </div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--space-xl); }
    section h2 { margin-bottom: var(--space-xs); }
    .summary { display: flex; flex-direction: column; gap: var(--space-m); }
    .measures { display: flex; flex-wrap: wrap; gap: var(--space-s) var(--space-l); }
    .group h3 { margin-bottom: var(--space-2xs); }
    /* Pull values left so they line up with the verdict; the severity rule sits in the gutter. */
    .group ml-readings { display: block; margin-left: -14px; }
    .delivery { flex: 0 0 260px; }
    .character { flex: 1 1 380px; max-width: 540px; }
    .verdict h2 { font-size: var(--text-title); display: flex; align-items: center; gap: var(--space-xs); }
    .verdict p { margin: 2px 0 0 18px; max-width: 80ch; }
    .bridge-head { display: flex; align-items: baseline; flex-wrap: wrap; column-gap: var(--space-s); margin-bottom: var(--space-xs); }
    .bridge-head p { font-size: var(--text-caption); }
    .inspect { margin-top: var(--space-s); border: 1px solid var(--line); border-radius: var(--radius-m); padding: var(--space-s) var(--space-m); background: var(--panel); }
    .inspect-head { display: flex; align-items: baseline; gap: var(--space-s); margin-bottom: var(--space-xs); }
    .inspect-head button { margin-left: auto; }
    .cols { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--space-xl); }
    .findings { list-style: none; margin: 0; padding: 0; }
    .findings li { display: grid; grid-template-columns: 14px 1fr; gap: var(--space-xs); padding: var(--space-s) 0; border-bottom: 1px solid var(--line-soft); }
    .findings li:first-child { padding-top: var(--space-2xs); }
    .findings p { color: var(--muted); max-width: 70ch; }
    .findings ml-sev { margin-top: 5px; }
    .passed { margin-top: var(--space-s); font-size: var(--text-body); color: var(--muted); }
    .findings .link { margin-top: var(--space-2xs); }
    .tip { display: flex; align-items: baseline; flex-wrap: wrap; gap: var(--space-2xs) var(--space-s); max-width: 80ch;
      padding: var(--space-xs) var(--space-s); border: 1px solid var(--line); border-radius: var(--radius-s); font-size: var(--text-caption); color: var(--muted); }
    .tip p { flex: 1 1 40ch; }
    .tip .ghost { font-size: var(--text-caption); padding: 2px 8px; }
    /* Findings and file details sit side by side only when the main column is wide enough. */
    @container (min-width: 820px) { .cols { grid-template-columns: minmax(0, 1.6fr) minmax(260px, 1fr); } }
  `,
})
export class Overview {
  readonly store = inject(Store);
  readonly num = num;
  readonly signed = signed;
  readonly time = time;
  readonly prose = prose;
  readonly r = computed(() => this.store.report()?.report ?? null);
  /** Findings that need reading; passed checks collapse into one line. */
  readonly issues = computed(() => this.r()?.findings.filter((f) => f.severity !== 'ok') ?? []);
  readonly passed = computed(() => this.r()?.findings.filter((f) => f.severity === 'ok').map((f) => prose(f.title)) ?? []);
  readonly isMaster = computed(() => this.store.report()?.kind === 'master');
  readonly target = computed(() => this.store.settings()?.effective_target_lufs ?? -14);
  readonly platformName = computed(() => this.store.platforms().find((p) => p.id === this.store.settings()?.main_platform)?.name ?? null);
  /** "Spotify target −14", or "Target −16" for a custom target. */
  readonly targetNote = computed(() => {
    const name = this.platformName();
    return `${name ? name + ' target' : 'Target'} ${num(this.target(), 0)}`;
  });

  /** Shown until dismissed: which rules ran, since the file name picked the type. */
  readonly kindTip = computed(() => {
    const kind = this.store.report()?.kind;
    if (this.store.kindTipSeen() || !kind || kind === 'reference') return null;
    if (kind === 'mix') {
      return 'MixLens checked this as a mix, so it looked at headroom and bus compression before mastering. If this is a finished master, switch to Master above to check it against your streaming target.';
    }
    const where = this.platformName() ? `${this.platformName()}'s` : 'your';
    return `MixLens checked this as a master against ${where} ${num(this.target(), 0)} LUFS target and true-peak ceiling.`;
  });

  /** One-line answer to "is this ready, and what do I fix first?", from the findings. */
  readonly verdict = computed(() => {
    const r = this.r();
    const kind = this.store.report()?.kind;
    if (!r || kind === 'reference') return null;
    const errors = r.findings.filter((f) => f.severity === 'error');
    const warns = r.findings.filter((f) => f.severity === 'warn');
    const ok = (code: string) => r.findings.some((f) => f.code === code && f.severity === 'ok');
    const goal = kind === 'master' ? 'before release' : 'before mastering';
    const where = this.platformName() ? `for ${this.platformName()}` : 'for your target';

    const top = errors[0] ?? warns[0];
    // What already passed, so a list of problems doesn't read as "nothing works".
    const already = top ? 'already ' : '';
    let passed = '';
    if (kind === 'master') {
      if (ok('loudness') && ok('tp_ceiling')) passed = `Loudness and true peak are ${already}on target ${where}.`;
      else if (ok('tp_ceiling')) passed = `True peak is ${already}under the ceiling ${where}.`;
      else if (ok('loudness')) passed = `Loudness is ${already}on target ${where}.`;
    } else if (ok('mix_headroom') && !r.findings.some((f) => f.code === 'mix_overcompressed')) {
      passed = 'Headroom and dynamics leave the master room to work.';
    }

    if (!top) return { sev: 'ok' as const, headline: 'No problems found', sub: passed };
    const n = errors.length || warns.length;
    const headline = errors.length
      ? `${n === 1 ? 'One problem' : `${n} problems`} to fix ${goal}`
      : `${n === 1 ? 'One thing' : `${n} things`} to check ${goal}`;
    return { sev: errors.length ? ('error' as const) : ('warn' as const), headline, sub: `Start with: ${prose(top.title)}. ${passed}`.trim() };
  });

  private sevFor(label: string, findings: Finding[]) {
    const codes = READING_CODES[label] ?? [];
    const hit = findings.filter((f) => codes.includes(f.code));
    if (!hit.length) return null;
    const order = ['ok', 'info', 'warn', 'error'];
    return hit.map((f) => f.severity).sort((a, b) => order.indexOf(b) - order.indexOf(a))[0];
  }

  readonly delivery = computed(() => this.readings().slice(0, 2));
  readonly character = computed(() => this.readings().slice(2));

  readonly readings = computed<Reading[]>(() => {
    const r = this.r();
    if (!r) return [];
    const l = r.loudness;
    const s = r.stereo;
    const f = r.findings;
    return [
      { label: 'Integrated', value: num(l.integrated_lufs), unit: 'LUFS', note: this.isMaster() ? this.targetNote() : undefined, severity: this.sevFor('Integrated', f) },
      { label: 'True peak', value: num(l.true_peak_dbtp), unit: 'dBTP', severity: this.sevFor('True peak', f) },
      { label: 'Loudness range', value: num(l.lra_lu), unit: 'LU' },
      { label: 'PLR', value: num(l.plr_db), unit: 'dB', note: 'peak to loudness', severity: this.sevFor('PLR', f) },
      { label: 'Mono fold', value: signed(s.mono_loss_lu, 2), unit: 'LU', severity: this.sevFor('Mono fold', f) },
      { label: 'Correlation', value: num(s.correlation, 2), note: `${num(s.low_correlation, 2)} below 120 Hz`, severity: this.sevFor('Low-end corr.', f) },
    ];
  });
}
