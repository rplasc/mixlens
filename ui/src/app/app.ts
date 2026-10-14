import { Component, OnInit, computed, inject, signal } from '@angular/core';
import { Kind } from './api';
import { Assistant } from './assistant';
import { Compare } from './compare';
import { format, time } from './fmt';
import { Overview } from './overview';
import { Settings } from './settings';
import { Sidebar } from './sidebar';
import { Spectrum } from './spectrum';
import { Stereo } from './stereo';
import { Store, Tab } from './store';

@Component({
  selector: 'app-root',
  imports: [Sidebar, Overview, Spectrum, Stereo, Compare, Assistant, Settings],
  template: `
    <div class="shell" [class.chat-closed]="!store.chatOpen()">
      <aside class="side"><ml-sidebar /></aside>

      <main>
        @if (store.version(); as v) {
          <header class="vhead">
            <div class="title">
              <p class="song">{{ store.project()?.name }}</p>
              @if (editing()) {
                <form (submit)="saveLabel($event, labelBox.value)">
                  <input #labelBox [value]="v.label" (blur)="saveLabel($event, labelBox.value)" (keydown.escape)="editing.set(false)" autofocus />
                </form>
              } @else {
                <h1 (dblclick)="editing.set(true)" title="Double-click to rename">{{ v.label }}</h1>
              }
              <p class="file" [title]="v.path">{{ v.file_name }}, {{ fmtFormat() }}, {{ time(v.duration_s) }}</p>
            </div>
            <div class="actions">
              <div class="seg" role="radiogroup" aria-label="Version type">
                <button role="radio" [attr.aria-checked]="v.kind === 'mix'" [class.on]="v.kind === 'mix'" (click)="setKind('mix')">Mix</button>
                <button role="radio" [attr.aria-checked]="v.kind === 'master'" [class.on]="v.kind === 'master'" (click)="setKind('master')">Master</button>
              </div>
              @if (confirmDelete()) {
                <button class="danger" (click)="store.deleteVersion(); confirmDelete.set(false)">Remove version</button>
                <button class="ghost" (click)="confirmDelete.set(false)">Keep</button>
              } @else {
                <button class="ghost" (click)="confirmDelete.set(true)">Remove</button>
              }
              <button class="ghost" (click)="store.chatOpen.set(!store.chatOpen())">{{ store.chatOpen() ? 'Hide assistant' : 'Assistant' }}</button>
            </div>
          </header>

          <nav class="tabs" role="tablist">
            @for (t of tabs; track t.id) {
              <button role="tab" [attr.aria-selected]="store.tab() === t.id" [class.on]="store.tab() === t.id" (click)="openTab(t.id)">{{ t.label }}</button>
            }
          </nav>

          <div class="content">
            @switch (store.tab()) {
              @case ('overview') { <ml-overview /> }
              @case ('spectrum') { <ml-spectrum /> }
              @case ('stereo') { <ml-stereo /> }
              @case ('compare') { <ml-compare /> }
            }
          </div>
        } @else {
          <div class="empty">
            @if (store.analyzing().length) {
              <h1>Analyzing {{ store.analyzing()[0] }}</h1>
              <p class="muted">Loudness, peaks, stereo, spectrum and edges, usually in a second or two.</p>
            } @else {
              <h1>Drop a mix or master here</h1>
              <p class="muted">WAV, FLAC, AIFF, MP3 or M4A. Each file becomes a version you can compare with the others.</p>
              <button class="primary" (click)="store.pickFiles()">Choose files</button>
            }
          </div>
        }
      </main>

      @if (store.chatOpen()) {
        <aside class="chat"><ml-assistant /></aside>
      }
    </div>

    @if (store.dragOver()) {
      <div class="drop" aria-hidden="true"><p>Drop to add {{ store.project() ? 'versions to ' + store.project()!.name : 'a new song' }}</p></div>
    }
    @if (store.error(); as e) {
      <div class="toast" role="alert">
        <p>{{ e }}</p>
        <button class="ghost" (click)="store.error.set(null)">Dismiss</button>
      </div>
    }
    @if (store.showSettings()) { <ml-settings /> }
  `,
  styles: `
    .shell { display: grid; grid-template-columns: 236px minmax(0, 1fr) 380px; height: 100vh; }
    .shell.chat-closed { grid-template-columns: 236px minmax(0, 1fr); }
    .side { border-right: 1px solid var(--line); background: #1e252b; min-height: 0; }
    .chat { border-left: 1px solid var(--line); background: #1e252b; min-height: 0; }
    main { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
    .vhead { display: flex; align-items: flex-end; justify-content: space-between; gap: 20px; padding: 18px 28px 12px; }
    .song { color: var(--muted); font-size: 13px; }
    .title h1 { font-size: 26px; cursor: text; }
    .title input { font-size: 22px; font-family: var(--font-display); width: 360px; }
    .file { color: var(--faint); font-size: 12px; margin-top: 2px; }
    .actions { display: flex; gap: 8px; align-items: center; }
    .seg { display: inline-flex; border: 1px solid var(--line); border-radius: var(--radius-s); overflow: hidden; }
    .seg button { border: none; border-radius: 0; background: transparent; padding: 5px 12px; }
    .seg button + button { border-left: 1px solid var(--line); }
    .seg button.on { background: var(--raised); color: var(--a); font-weight: 600; }
    .danger { border-color: var(--err); color: var(--err); background: transparent; }
    .tabs { display: flex; gap: 2px; padding: 0 22px; border-bottom: 1px solid var(--line); }
    .tabs button { background: transparent; border: none; border-bottom: 2px solid transparent; border-radius: 0; padding: 8px 10px; color: var(--muted); }
    .tabs button.on { color: var(--ink); border-bottom-color: var(--a); }
    .tabs button:hover { color: var(--ink); }
    .content { flex: 1; overflow-y: auto; padding: 18px 28px 40px; }
    .empty { margin: auto; text-align: center; display: flex; flex-direction: column; gap: 10px; align-items: center; max-width: 440px; padding: 24px; }
    .empty h1 { font-size: 26px; }
    .drop { position: fixed; inset: 10px; border: 2px dashed var(--a); border-radius: 12px; background: rgba(27, 33, 39, 0.82);
      display: grid; place-items: center; z-index: 30; pointer-events: none; font-size: 20px; font-family: var(--font-display); }
    .toast { position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); z-index: 40; display: flex; align-items: center; gap: 12px;
      background: var(--raised); border: 1px solid var(--err); border-radius: var(--radius-m); padding: 10px 12px 10px 16px; max-width: min(720px, 90vw); }
    @media (max-width: 1200px) { .shell { grid-template-columns: 220px minmax(0, 1fr) 320px; } }
  `,
})
export class App implements OnInit {
  readonly store = inject(Store);
  readonly editing = signal(false);
  readonly confirmDelete = signal(false);
  readonly time = time;
  readonly tabs: { id: Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'spectrum', label: 'Tone' },
    { id: 'stereo', label: 'Stereo' },
    { id: 'compare', label: 'Compare versions' },
  ];

  readonly fmtFormat = computed(() => {
    const v = this.store.version();
    return v ? format(v.sample_format, v.sample_rate) : '';
  });

  ngOnInit() {
    void this.store.init();
  }

  openTab(t: Tab) {
    this.store.tab.set(t);
    if (t === 'compare' && !this.store.comparison() && this.store.compareWith() !== null) void this.store.runCompare();
  }

  setKind(kind: Kind) {
    const v = this.store.version();
    if (v && v.kind !== kind) void this.store.updateVersion(v.label, kind);
  }

  saveLabel(ev: Event, label: string) {
    ev.preventDefault();
    if (!this.editing()) return;
    this.editing.set(false);
    const v = this.store.version();
    if (v && label.trim() && label.trim() !== v.label) void this.store.updateVersion(label.trim(), v.kind);
  }
}
