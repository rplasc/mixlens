import { Injectable, computed, signal } from '@angular/core';
import { api, errorText } from './api';

/** The open version (gold in the Compare tab) or the one it's compared with (cyan). */
export type Side = 'current' | 'partner';

/** What the A/B player plays, as the Compare tab measured it. */
export interface PlayPair {
  /** Versions of one song play in sync; a reference is another song, so each side keeps its own playhead. */
  mode: 'version' | 'reference';
  current: number;
  partner: number;
  /** The partner's time is the current version's time minus this (version mode). */
  lagS: number;
  /** Partner minus current integrated loudness (LU); not finite when either is silent. */
  loudnessDiff: number;
  durations: Record<Side, number>;
}

/** Short fades so switching, seeking and pausing never click. */
const FADE_S = 0.015;
const SIDES: Side[] = ['current', 'partner'];

interface Voice { src: AudioBufferSourceNode; gain: GainNode }

/**
 * Level-matched A/B playback. Matching only ever turns the louder side down, so it can't
 * push either file into clipping.
 */
@Injectable({ providedIn: 'root' })
export class Player {
  readonly pair = signal<PlayPair | null>(null);
  readonly side = signal<Side>('current');
  readonly playing = signal(false);
  readonly loading = signal(false);
  readonly error = signal<string | null>(null);
  readonly levelMatch = signal(true);
  /** Played, moved or switched since this pair opened, so views can show the playhead and the audible side. */
  readonly touched = signal(false);
  /** Version mode: time on the open version's timeline. Reference mode: the audible side's own time. */
  readonly position = signal(0);

  readonly matchable = computed(() => Number.isFinite(this.pair()?.loudnessDiff ?? NaN));
  /** Gain applied to each side (dB, never above 0). */
  readonly gainDb = computed<Record<Side, number>>(() => {
    const d = this.pair()?.loudnessDiff ?? NaN;
    if (!this.levelMatch() || !Number.isFinite(d)) return { current: 0, partner: 0 };
    return { current: Math.min(0, d), partner: Math.min(0, -d) };
  });
  readonly duration = computed(() => {
    const p = this.pair();
    if (!p) return 0;
    return p.mode === 'version' ? Math.max(p.durations.current, p.durations.partner + p.lagS) : p.durations[this.side()];
  });

  private ctx: AudioContext | null = null;
  /** Decoded buffers for the pair in view, by version id. */
  private readonly buffers = new Map<number, AudioBuffer>();
  private voices: Partial<Record<Side, Voice>> = {};
  /** Each side's position, in its own seconds, when playback last started. */
  private offset: Record<Side, number> = { current: 0, partner: 0 };
  private startedAt = 0;
  private raf = 0;
  /** Identifies the pair, so a comparison that re-runs keeps its positions. */
  private key = '';

  setPair(p: PlayPair | null) {
    const key = p ? `${p.mode}:${p.current}:${p.partner}:${p.lagS}` : this.key;
    if (key !== this.key || !p) this.pause();
    this.pair.set(p);
    if (key !== this.key) {
      this.key = key;
      // Free the previous pair's audio; a 4-minute song is about 90 MB decoded.
      for (const id of [...this.buffers.keys()]) if (id !== p?.current && id !== p?.partner) this.buffers.delete(id);
      this.side.set('current');
      this.touched.set(false);
      this.error.set(null);
      this.offset = { current: 0, partner: 0 };
      this.place(0);
    }
    // A re-run comparison can measure a slightly different loudness.
    this.applyGains();
  }

  async toggle() {
    if (this.playing()) this.pause();
    else await this.play();
  }

  async play() {
    const p = this.pair();
    if (!p || this.playing() || this.loading()) return;
    const key = this.key;
    // Created inside the click that asked for sound, so the webview lets it play.
    this.ctx ??= new AudioContext();
    const ctx = this.ctx;
    this.error.set(null);
    try {
      await ctx.resume();
      await this.load(ctx, p);
    } catch (e) {
      this.error.set(errorText(e));
      return;
    } finally {
      this.loading.set(false);
    }
    // Another comparison may have opened while the audio loaded.
    if (this.key !== key || this.playing()) return;
    if (this.position() >= this.duration() - 0.05) this.seek(0);
    this.start();
  }

  pause() {
    if (!this.playing() || !this.ctx) return;
    const t = this.livePosition(Math.max(0, this.ctx.currentTime - this.startedAt));
    this.stopVoices();
    this.place(t);
  }

  /** Jump to `t` seconds (on the timeline `position` uses). */
  seek(t: number) {
    if (!this.pair()) return;
    this.touched.set(true);
    const was = this.playing();
    this.stopVoices();
    this.place(Math.max(0, Math.min(t, this.duration())));
    if (was) this.start();
  }

  /** Set the playhead while stopped: both sides in sync mode, the audible one otherwise. */
  private place(t: number) {
    const p = this.pair();
    if (p?.mode === 'version') this.offset = { current: t, partner: t - p.lagS };
    else this.offset[this.side()] = t;
    this.position.set(t);
  }

  switchTo(side: Side) {
    const p = this.pair();
    if (!p || side === this.side()) return;
    this.touched.set(true);
    if (p.mode === 'version' || !this.playing()) {
      // In sync mode both sides are already running; only their gains change.
      this.side.set(side);
      if (p.mode === 'reference') this.position.set(this.offset[side]);
      this.applyGains();
      return;
    }
    this.pause();
    this.side.set(side);
    this.position.set(this.offset[side]);
    this.start();
  }

  setLevelMatch(on: boolean) {
    this.levelMatch.set(on);
    this.applyGains();
  }

  private async load(ctx: AudioContext, p: PlayPair) {
    const missing = [p.current, p.partner].filter((id) => !this.buffers.has(id));
    if (!missing.length) return;
    this.loading.set(true);
    const data = await Promise.all(missing.map((id) => api.playbackAudio(id)));
    data.forEach((bytes, i) => {
      if (!bytes) throw new Error('Playback runs in the desktop app.');
      this.buffers.set(missing[i], toBuffer(ctx, bytes));
    });
  }

  private start() {
    const p = this.pair();
    const ctx = this.ctx;
    if (!p || !ctx) return;
    const when = ctx.currentTime + 0.02;
    const gains = this.targetGains();
    for (const s of p.mode === 'version' ? SIDES : [this.side()]) {
      const buf = this.buffers.get(p[s]);
      const at = this.offset[s];
      if (!buf || at >= buf.duration) continue;
      const gain = ctx.createGain();
      gain.gain.setValueAtTime(0, when);
      gain.gain.linearRampToValueAtTime(gains[s], when + FADE_S);
      gain.connect(ctx.destination);
      const src = ctx.createBufferSource();
      src.buffer = buf;
      src.connect(gain);
      // A side that starts later on the shared timeline waits for its first sample.
      if (at < 0) src.start(when - at, 0);
      else src.start(when, at);
      this.voices[s] = { src, gain };
    }
    this.startedAt = when;
    this.playing.set(true);
    this.touched.set(true);
    this.raf = requestAnimationFrame(this.tick);
  }

  private stopVoices() {
    cancelAnimationFrame(this.raf);
    const ctx = this.ctx;
    if (ctx) {
      const now = ctx.currentTime;
      for (const v of Object.values(this.voices)) {
        v.gain.gain.cancelScheduledValues(now);
        v.gain.gain.setValueAtTime(v.gain.gain.value, now);
        v.gain.gain.linearRampToValueAtTime(0, now + FADE_S);
        v.src.stop(now + FADE_S);
        v.src.onended = () => v.gain.disconnect();
      }
    }
    this.voices = {};
    this.playing.set(false);
  }

  /** Linear gain per side: the audible side at its matched level, the other silent. */
  private targetGains(): Record<Side, number> {
    const db = this.gainDb();
    const lin = (s: Side) => (s === this.side() ? 10 ** (db[s] / 20) : 0);
    return { current: lin('current'), partner: lin('partner') };
  }

  private applyGains() {
    const ctx = this.ctx;
    if (!ctx) return;
    const now = ctx.currentTime;
    const gains = this.targetGains();
    for (const [s, v] of Object.entries(this.voices) as [Side, Voice][]) {
      v.gain.gain.cancelScheduledValues(now);
      v.gain.gain.setValueAtTime(v.gain.gain.value, now);
      v.gain.gain.linearRampToValueAtTime(gains[s], now + FADE_S);
    }
  }

  private livePosition(elapsed: number) {
    const p = this.pair();
    if (!p) return 0;
    return (p.mode === 'version' ? this.offset.current : this.offset[this.side()]) + elapsed;
  }

  private readonly tick = () => {
    if (!this.playing() || !this.ctx) return;
    const pos = this.livePosition(Math.max(0, this.ctx.currentTime - this.startedAt));
    if (pos >= this.duration()) {
      this.stopVoices();
      this.seek(0);
      return;
    }
    this.position.set(pos);
    this.raf = requestAnimationFrame(this.tick);
  };
}

/** Header: sample rate and frame count (u32 LE); then left, then right, as f32 (little-endian, like every host this runs on). */
function toBuffer(ctx: AudioContext, bytes: ArrayBuffer): AudioBuffer {
  const head = new DataView(bytes, 0, 8);
  const rate = head.getUint32(0, true);
  const n = head.getUint32(4, true);
  const buf = ctx.createBuffer(2, Math.max(1, n), rate);
  buf.copyToChannel(new Float32Array(bytes, 8, n), 0);
  buf.copyToChannel(new Float32Array(bytes, 8 + n * 4, n), 1);
  return buf;
}
