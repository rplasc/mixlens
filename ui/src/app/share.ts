import { Finding, Kind, Severity } from './api';
import { prose } from './fmt';

/** Same groups and headings as the Findings section of the exported report (`export.rs`). */
const GROUPS: [Severity, string][] = [
  ['error', 'Problems'],
  ['warn', 'Checks'],
  ['info', 'Notes'],
];

export interface ShareInput {
  song: string;
  version: string;
  kind: Kind;
  verdict: { headline: string; sub: string } | null;
  /** Key numbers, e.g. "−9.8 LUFS integrated". */
  readings: string[];
  findings: Finding[];
}

/** Rules-engine prose without the invisible word joiners, which paste badly into chat and email. */
const plain = (s: string) => prose(s).replace(/⁠/g, '');

/** Verdict and findings as plain text, for pasting into a chat message or an email. */
export function findingsText(x: ShareInput): string {
  const lines = [`${x.song} – ${x.version} (${x.kind})`];
  if (x.verdict) lines.push([`${x.verdict.headline}.`, x.verdict.sub].filter(Boolean).join(' '));
  if (x.readings.length) lines.push(x.readings.join(' · '));
  for (const [sev, heading] of GROUPS) {
    const group = x.findings.filter((f) => f.severity === sev);
    if (group.length) lines.push('', heading, ...group.map((f) => `- ${plain(f.title)}. ${plain(f.detail)}`));
  }
  const passed = x.findings.filter((f) => f.severity === 'ok').map((f) => plain(f.title));
  if (passed.length) lines.push('', `Passed: ${passed.join(', ')}`);
  return lines.join('\n');
}
