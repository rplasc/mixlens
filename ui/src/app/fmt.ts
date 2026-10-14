export function time(s: number): string {
  if (!isFinite(s)) return '–';
  const m = Math.floor(s / 60);
  const sec = s - m * 60;
  return `${m}:${sec < 10 ? '0' : ''}${sec.toFixed(sec % 1 === 0 ? 0 : 1)}`;
}

export function clock(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.floor(s - m * 60);
  return `${m}:${sec.toString().padStart(2, '0')}`;
}

function clean(v: number, digits: number): string {
  const s = v.toFixed(digits);
  return Number(s) === 0 ? (0).toFixed(digits) : s;
}

export function num(v: number | null | undefined, digits = 1): string {
  if (v == null || !isFinite(v)) return '–';
  return clean(v, digits).replace('-', '−');
}

export function signed(v: number | null | undefined, digits = 1): string {
  if (v == null || !isFinite(v)) return '–';
  const s = clean(v, digits);
  return (Number(s) > 0 ? '+' : '') + s.replace('-', '−');
}

/** Typographic minus for negative numbers in prose from the rules engine ("-14.0" to "−14.0"). */
export function prose(s: string): string {
  // A word joiner around range dashes keeps "20–40 Hz" on one line.
  return s.replace(/(^|[\s(])-(?=\d)/g, '$1−').replace(/(\d)–(\d)/g, '$1\u2060–\u2060$2');
}

export function hz(f: number): string {
  return f >= 1000 ? `${+(f / 1000).toFixed(f >= 10000 ? 0 : 1)}k` : `${Math.round(f)}`;
}

export function format(sampleFormat: string, rate: number): string {
  const depth = sampleFormat.replace('int', '').replace('float', '');
  const kind = sampleFormat.startsWith('float') ? `${depth}-bit float` : sampleFormat.startsWith('int') ? `${depth}-bit` : sampleFormat;
  return `${(rate / 1000).toFixed(rate % 1000 ? 1 : 0)} kHz, ${kind}`;
}

export const TONICS = ['C', 'C♯/D♭', 'D', 'E♭', 'E', 'F', 'F♯', 'G', 'A♭', 'A', 'B♭', 'B'];

export function keyName(k: { tonic: number; mode: 'major' | 'minor' } | null | undefined): string {
  return k ? `${TONICS[k.tonic]} ${k.mode}` : '';
}
