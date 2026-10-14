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

export function hz(f: number): string {
  return f >= 1000 ? `${+(f / 1000).toFixed(f >= 10000 ? 0 : 1)}k` : `${Math.round(f)}`;
}

export function format(sampleFormat: string, rate: number): string {
  const depth = sampleFormat.replace('int', '').replace('float', '');
  const kind = sampleFormat.startsWith('float') ? `${depth}-bit float` : sampleFormat.startsWith('int') ? `${depth}-bit` : sampleFormat;
  return `${(rate / 1000).toFixed(rate % 1000 ? 1 : 0)} kHz, ${kind}`;
}
