import { Component, ElementRef, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { ExportFormat, Kind, MusicalKey } from './api';
import { Assistant } from './assistant';
import { Compare } from './compare';
import { Autofocus } from './dom';
import { format, keyName, time, TONICS } from './fmt';
import { Overview } from './overview';
import { Settings } from './settings';
import { Sidebar } from './sidebar';
import { Spectrum } from './spectrum';
import { Stereo } from './stereo';
import { Streaming } from './streaming';
import { Store, Tab } from './store';

/** Wide enough for the assistant to sit beside the main column; matches the 999 px breakpoint. */
const WIDE = '(min-width: 1000px)';

@Component({
  selector: 'app-root',
  imports: [NgTemplateOutlet, Autofocus, Sidebar, Overview, Spectrum, Stereo, Streaming, Compare, Assistant, Settings],
  host: { '(document:keydown.escape)': 'closeOverlays()' },
  template: `
    <div class="shell" [class.chat-closed]="!store.chatOpen()" [class.nav-open]="store.navOpen()">
      <aside class="side" id="side-panel" aria-label="Songs and versions"><ml-sidebar /></aside>

      <main>
        @if (store.version(); as v) {
          <header class="vhead">
            <div class="top">
              <div class="lead">
                <ng-container *ngTemplateOutlet="navToggle" />
                <p class="song">{{ store.project()?.name }}</p>
              </div>
              <div class="actions">
                <div class="seg" role="group" aria-label="Version type">
                  <button [attr.aria-pressed]="v.kind === 'mix'" [class.on]="v.kind === 'mix'" (click)="setKind('mix')">Mix</button>
                  <button [attr.aria-pressed]="v.kind === 'master'" [class.on]="v.kind === 'master'" (click)="setKind('master')">Master</button>
                  <button [attr.aria-pressed]="v.kind === 'reference'" [class.on]="v.kind === 'reference'" (click)="setKind('reference')"
                    title="Someone else's finished track, used for comparisons">Reference</button>
                </div>
                <div class="export" #exportMenu (focusout)="exportFocusOut($event)">
                  <button class="ghost" aria-haspopup="menu" aria-controls="export-menu" [attr.aria-expanded]="exportOpen()"
                    [disabled]="store.exportState() === 'busy'" (click)="toggleExport()" (keydown)="exportButtonKey($event)">
                    {{ exportLabel() }}
                    <svg class="caret" width="8" height="5" viewBox="0 0 8 5" aria-hidden="true"><path d="M0.5 0.5 4 4 7.5 0.5" /></svg>
                  </button>
                  @if (exportOpen()) {
                    <div id="export-menu" role="menu" aria-label="Export report as" (keydown)="exportMenuKey($event)">
                      <button role="menuitem" tabindex="-1" (click)="exportAs('pdf')" (mouseenter)="$any($event.target).focus()">PDF<span>A formatted page to send to a client or label</span></button>
                      <button role="menuitem" tabindex="-1" (click)="exportAs('markdown')" (mouseenter)="$any($event.target).focus()">Markdown<span>Plain text to paste into an email or notes</span></button>
                    </div>
                  }
                </div>
                <button class="ghost toggle" [class.on]="store.chatOpen()" [attr.aria-pressed]="store.chatOpen()"
                  (click)="store.chatOpen.set(!store.chatOpen())">Assistant</button>
              </div>
            </div>

            <div class="name">
              @if (editing()) {
                <form (submit)="saveLabel($event, labelBox.value)">
                  <input #labelBox aria-label="Version name" [value]="v.label" (blur)="saveLabel($event, labelBox.value)" (keydown.escape)="editing.set(false)" mlAutofocus />
                </form>
              } @else {
                <h1 (dblclick)="editing.set(true)">{{ v.label }}</h1>
                <button class="ghost rename" (click)="editing.set(true)" [attr.aria-label]="'Rename ' + v.label">Rename</button>
              }
              <p class="file" [title]="v.path">{{ fileLine() }}</p>
            </div>

            <div class="context">
              @if (store.context().reference) { <span class="ctx-scope" title="A reference is another song, so it has its own genre and key">This reference</span> }
              <label class="ctx">
                <span>Genre</span>
                <select (change)="setGenre($any($event.target).value)" [attr.aria-label]="scope() + ' genre'">
                  <option value="" [selected]="!store.context().genre">Not set</option>
                  @for (g of store.presets(); track g.id) {
                    <option [value]="g.id" [selected]="g.id === store.context().genre">{{ g.name }}</option>
                  }
                </select>
              </label>
              <label class="ctx">
                <span>Key</span>
                <select (change)="setTonic($any($event.target).value)" [attr.aria-label]="scope() + ' key'">
                  <option value="" [selected]="!key()">Not set</option>
                  @for (n of tonics; track $index) {
                    <option [value]="$index" [selected]="key()?.tonic === $index">{{ n }}</option>
                  }
                </select>
                <select (change)="setMode($any($event.target).value)" [disabled]="!key()" aria-label="Major or minor">
                  <option value="major" [selected]="key()?.mode !== 'minor'">major</option>
                  <option value="minor" [selected]="key()?.mode === 'minor'">minor</option>
                </select>
              </label>
              @if (store.context().reference) {
                <span class="hint">Song: {{ songContext() }}</span>
              } @else if (!key()) {
                <span class="hint">Set the key to name bass notes.</span>
              }
            </div>
          </header>

          <div class="tabs" role="tablist" aria-label="Views" (keydown)="tabKey($event)">
            @for (t of tabs; track t.id) {
              <button role="tab" [id]="'tab-' + t.id" aria-controls="tab-panel" [attr.aria-selected]="store.tab() === t.id"
                [attr.tabindex]="store.tab() === t.id ? 0 : -1" [class.on]="store.tab() === t.id" (click)="openTab(t.id)">{{ t.label }}</button>
            }
          </div>

          <div class="content" id="tab-panel" role="tabpanel" [attr.aria-labelledby]="'tab-' + store.tab()" tabindex="0">
            @switch (store.tab()) {
              @case ('overview') { <ml-overview /> }
              @case ('spectrum') { <ml-spectrum /> }
              @case ('stereo') { <ml-stereo /> }
              @case ('streaming') { <ml-streaming /> }
              @case ('compare') { <ml-compare /> }
            }
          </div>
        } @else {
          <div class="floating"><ng-container *ngTemplateOutlet="navToggle" /></div>
          <div class="empty">
            @if (store.analyzing().length) {
              <h1>Analyzing {{ store.analyzing()[0] }}</h1>
              <p class="muted">Measuring loudness, peaks, stereo and tone. This usually takes a second or two.</p>
            } @else {
              <h1>Drop a mix or master here</h1>
              <p class="muted">WAV, FLAC, AIFF, MP3 or M4A. MixLens analyzes each file on this computer in a second or two.</p>
              <button class="primary" (click)="store.pickFiles()">Choose files</button>
              <ul class="gets" aria-label="What MixLens checks">
                <li>What to fix first, before release or mastering</li>
                <li>Loudness and peaks against your streaming target</li>
                <li>Stereo, tone and how the file starts and ends</li>
                <li>What changed between two bounces, second by second</li>
              </ul>
              <p class="naming">MixLens checks files with "master" in the name as masters and treats "ref" files as references. You can change either one later.</p>
            }
          </div>
        }
      </main>

      @if (store.chatOpen()) {
        <!-- Loaded on first open: keeps the assistant and its markdown parser out of the startup bundle. -->
        <aside class="chat" aria-label="Assistant">@defer (on immediate) { <ml-assistant /> }</aside>
      }
      <!-- Shown only when a panel overlays the main column (narrow windows or high zoom). -->
      <div class="scrim" aria-hidden="true" (click)="closeOverlays()"></div>
    </div>

    <ng-template #navToggle>
      <button class="nav-toggle" (click)="openNav()" [attr.aria-expanded]="store.navOpen()" aria-controls="side-panel">Songs</button>
    </ng-template>

    @if (store.dragOver()) {
      <div class="drop" aria-hidden="true"><p>Drop to add {{ store.project() ? 'versions to ' + store.project()!.name : 'a new song' }}</p></div>
    }
    @if (store.error(); as e) {
      <div class="toast" role="alert">
        <p>{{ e }}</p>
        <button class="ghost" (click)="store.error.set(null)">Dismiss</button>
      </div>
    }
    @if (store.showSettings()) { @defer (on immediate) { <ml-settings /> } }
  `,
  styles: `
    .shell { display: grid; grid-template-columns: 236px minmax(0, 1fr) 380px; height: 100vh; position: relative; }
    .shell.chat-closed { grid-template-columns: 236px minmax(0, 1fr); }
    .side { border-right: 1px solid var(--line); background: var(--surface-side); min-height: 0; }
    .chat { border-left: 1px solid var(--line); background: var(--surface-side); min-height: 0; }
    main { display: flex; flex-direction: column; min-width: 0; min-height: 0; position: relative; }
    .lead { display: flex; align-items: center; gap: var(--space-xs); min-width: 0; }
    .nav-toggle { display: none; }
    .floating { position: absolute; top: var(--space-s); left: var(--space-s); }
    .scrim { display: none; position: fixed; inset: 0; z-index: var(--z-scrim); background: var(--scrim); }
    .vhead { display: flex; flex-direction: column; padding: var(--space-m) var(--page-x) var(--space-s); max-width: calc(1600px + 2 * var(--page-x)); }
    .top { display: flex; align-items: center; justify-content: space-between; gap: var(--space-m); }
    .song { color: var(--muted); font-size: var(--text-caption); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .actions { display: flex; gap: var(--space-xs); align-items: center; flex: none; }
    .toggle { white-space: nowrap; }
    .export { position: relative; }
    .export > button { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
    .caret { fill: none; stroke: currentColor; stroke-width: 1.3; stroke-linecap: round; stroke-linejoin: round; }
    [role='menu'] { position: absolute; top: calc(100% + 4px); right: 0; z-index: var(--z-menu); width: max-content; max-width: min(300px, 90vw);
      display: flex; flex-direction: column; padding: 4px; background: var(--raised); border: 1px solid var(--line);
      border-radius: var(--radius-m); box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35); }
    [role='menuitem'] { display: grid; gap: 1px; text-align: left; border: none; border-radius: var(--radius-s); background: transparent; padding: 6px 10px; }
    [role='menuitem'] span { font-size: var(--text-caption); color: var(--muted); }
    /* Focus follows the pointer (mouseenter), so the focused item is the one highlight, from mouse or keys. */
    [role='menuitem']:focus { background: var(--line); }
    [role='menuitem']:focus-visible { outline-offset: -2px; }
    .toggle.on { color: var(--ink); border-color: var(--line); }
    .name { display: flex; align-items: baseline; flex-wrap: wrap; column-gap: var(--space-s); row-gap: var(--space-2xs); margin-top: var(--space-2xs); }
    .name h1 { font-size: var(--text-heading); cursor: text; min-width: 0; overflow-wrap: anywhere; }
    .rename { font-size: var(--text-caption); padding: 2px 8px; }
    .name input { font-size: var(--text-heading); font-family: var(--font-display); width: min(360px, 100%); }
    .file { color: var(--faint); font-size: var(--text-caption); }
    .context { display: flex; align-items: center; flex-wrap: wrap; gap: var(--space-xs) var(--space-l); margin-top: var(--space-s); }
    .ctx { display: inline-flex; align-items: center; gap: var(--space-xs); font-size: var(--text-body); color: var(--muted); }
    .ctx select { padding: 3px 6px; }
    .ctx-scope { font-size: var(--text-caption); color: var(--b); border: 1px solid var(--b); border-radius: 3px; padding: 0 5px; }
    .hint { font-size: var(--text-caption); color: var(--faint); }
    .tabs { display: flex; gap: 2px; padding: 0 calc(var(--page-x) - 10px); border-bottom: 1px solid var(--line); }
    .tabs button { background: transparent; border: none; border-bottom: 2px solid transparent; border-radius: 0; padding: 8px 10px; color: var(--muted); }
    .tabs button.on { color: var(--ink); border-bottom-color: var(--ink); }
    .tabs button:hover { color: var(--ink); }
    .content { flex: 1; overflow-y: auto; padding: var(--space-l) var(--page-x) var(--space-2xl); container-type: inline-size; }
    .content:focus-visible { outline-offset: -2px; }
    /* Very wide monitors: keep line lengths and charts at a readable size. */
    .content > * { max-width: 1600px; }
    .empty { margin: auto; text-align: center; display: flex; flex-direction: column; gap: 10px; align-items: center; max-width: 440px; padding: var(--space-l); }
    .gets { list-style: none; margin: var(--space-m) 0 0; padding: var(--space-m) 0 0; border-top: 1px solid var(--line-soft);
      display: grid; gap: var(--space-2xs); color: var(--muted); }
    .naming { font-size: var(--text-caption); color: var(--faint); margin-top: var(--space-xs); }
    .drop { position: fixed; inset: 10px; border: 2px dashed var(--a); border-radius: 12px; background: color-mix(in srgb, var(--bg) 82%, transparent);
      display: grid; place-items: center; z-index: var(--z-drop); pointer-events: none; font-size: var(--text-title); font-family: var(--font-display); }
    .toast { position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); z-index: var(--z-toast); display: flex; align-items: center; gap: var(--space-s);
      background: var(--raised); border: 1px solid var(--err); border-radius: var(--radius-m); padding: 10px 12px 10px 16px; max-width: min(720px, 90vw); }
    /* Breakpoints are in CSS pixels, so they also apply when the window is zoomed in. */
    @media (max-width: 1199px) { .shell { grid-template-columns: 220px minmax(0, 1fr) 320px; } }
    /* Below 1000 px the assistant slides over the main column instead of squeezing it. */
    @media (max-width: 999px) {
      .shell, .shell.chat-closed { grid-template-columns: 220px minmax(0, 1fr); }
      .chat { position: fixed; top: 0; right: 0; bottom: 0; width: min(380px, calc(100vw - 48px)); z-index: var(--z-drawer); }
      .shell:not(.chat-closed) .scrim { display: block; }
    }
    /* Below 720 px the song list becomes a drawer too, opened from the Songs button. */
    @media (max-width: 719px) {
      .shell, .shell.chat-closed { grid-template-columns: minmax(0, 1fr); }
      .side { position: fixed; top: 0; left: 0; bottom: 0; width: min(260px, calc(100vw - 48px)); z-index: var(--z-drawer);
        transform: translateX(-100%); visibility: hidden; transition: transform 200ms cubic-bezier(0.25, 1, 0.5, 1), visibility 200ms; }
      .shell.nav-open .side { transform: none; visibility: visible; }
      .shell.nav-open .scrim { display: block; }
      .nav-toggle { display: inline-flex; }
    }
  `,
})
export class App implements OnInit {
  readonly store = inject(Store);
  readonly editing = signal(false);
  readonly tabs: { id: Tab; label: string }[] = [
    { id: 'overview', label: 'Overview' },
    { id: 'spectrum', label: 'Tone' },
    { id: 'stereo', label: 'Stereo' },
    { id: 'streaming', label: 'Streaming' },
    { id: 'compare', label: 'Compare versions' },
  ];

  readonly tonics = TONICS;
  readonly key = computed(() => this.store.context().key);
  readonly exportLabel = computed(() => ({ idle: 'Export report', busy: 'Exporting…', saved: 'Saved' })[this.store.exportState()]);
  readonly exportOpen = signal(false);
  private readonly exportMenu = viewChild<ElementRef<HTMLElement>>('exportMenu');
  readonly scope = computed(() => (this.store.context().reference ? 'Reference' : 'Song'));
  readonly songContext = computed(() => {
    const p = this.store.project();
    const genre = this.store.presets().find((g) => g.id === p?.genre)?.name;
    return [genre ?? 'no genre', p?.key ? keyName(p.key) : 'no key'].join(', ');
  });

  setGenre(id: string) {
    void this.store.setContext(id || null, this.key());
  }

  setTonic(v: string) {
    const key: MusicalKey | null = v === '' ? null : { tonic: +v, mode: this.key()?.mode ?? 'major' };
    void this.store.setContext(this.store.context().genre, key);
  }

  setMode(mode: string) {
    const k = this.key();
    if (k) void this.store.setContext(this.store.context().genre, { ...k, mode: mode === 'minor' ? 'minor' : 'major' });
  }

  /** File details under the title; the file name only when the label isn't already it. */
  readonly fileLine = computed(() => {
    const v = this.store.version();
    if (!v) return '';
    const sameName = v.label === v.file_name.replace(/\.[^.]+$/, '');
    return [sameName ? null : v.file_name, format(v.sample_format, v.sample_rate), time(v.duration_s)].filter(Boolean).join(', ');
  });

  ngOnInit() {
    void this.store.init();
  }

  openNav() {
    this.store.navOpen.set(true);
    // Move focus into the drawer once it's visible.
    setTimeout(() => document.querySelector<HTMLElement>('#side-panel button')?.focus());
  }

  private exportItems(): HTMLElement[] {
    return Array.from(this.exportMenu()?.nativeElement.querySelectorAll<HTMLElement>('[role=menuitem]') ?? []);
  }

  /** Opening moves focus to the first format, as a menu button should. */
  toggleExport(focus: 'first' | 'last' = 'first') {
    this.exportOpen.update((o) => !o);
    if (this.exportOpen()) setTimeout(() => (focus === 'first' ? this.exportItems()[0] : this.exportItems().at(-1))?.focus());
  }

  closeExport(refocus: boolean) {
    this.exportOpen.set(false);
    if (refocus) this.exportMenu()?.nativeElement.querySelector<HTMLElement>('button')?.focus();
  }

  exportAs(format: ExportFormat) {
    this.closeExport(true);
    void this.store.exportReport(format);
  }

  exportButtonKey(ev: KeyboardEvent) {
    if (this.exportOpen() || (ev.key !== 'ArrowDown' && ev.key !== 'ArrowUp')) return;
    ev.preventDefault();
    this.toggleExport(ev.key === 'ArrowDown' ? 'first' : 'last');
  }

  /** Arrow keys, Home and End move between formats; Tab leaves and closes the menu. */
  exportMenuKey(ev: KeyboardEvent) {
    const items = this.exportItems();
    const n = items.length;
    const i = items.indexOf(document.activeElement as HTMLElement);
    const moves: Record<string, number> = { ArrowDown: i + 1, ArrowUp: i - 1 + n, Home: 0, End: n - 1 };
    if (!(ev.key in moves)) return;
    ev.preventDefault();
    items[moves[ev.key] % n].focus();
  }

  /** Focus leaving the button and menu (Tab, or a click elsewhere) closes the menu. */
  exportFocusOut(ev: FocusEvent) {
    if (!this.exportMenu()?.nativeElement.contains(ev.relatedTarget as Node | null)) this.exportOpen.set(false);
  }

  /** Close whatever is overlaying the main column: the export menu, the song drawer, or the assistant when it's a drawer. */
  closeOverlays() {
    // Escape in Settings closes only the dialog, which handles it itself.
    if (this.store.showSettings()) return;
    if (this.exportOpen()) {
      this.closeExport(true);
    } else if (this.store.navOpen()) {
      this.store.navOpen.set(false);
      document.querySelector<HTMLElement>('.nav-toggle')?.focus();
    } else if (this.store.chatOpen() && !matchMedia(WIDE).matches) {
      this.store.chatOpen.set(false);
    }
  }

  /** Arrow keys, Home and End move between tabs (one Tab stop for the whole row). */
  tabKey(ev: KeyboardEvent) {
    const n = this.tabs.length;
    const i = this.tabs.findIndex((t) => t.id === this.store.tab());
    const moves: Record<string, number> = { ArrowRight: i + 1, ArrowLeft: i - 1 + n, Home: 0, End: n - 1 };
    if (!(ev.key in moves)) return;
    ev.preventDefault();
    const t = this.tabs[moves[ev.key] % n];
    this.openTab(t.id);
    document.getElementById('tab-' + t.id)?.focus();
  }

  openTab(t: Tab) {
    this.store.tab.set(t);
    if (t === 'compare' && !this.store.comparison() && !this.store.refComparison() && this.store.compareWith() !== null) void this.store.runCompare();
  }

  setKind(kind: Kind) {
    this.store.dismissKindTip();
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
