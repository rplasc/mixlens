import { AfterViewChecked, Component, ElementRef, computed, inject, signal, viewChild } from '@angular/core';
import { marked } from 'marked';
import { Store } from './store';

marked.setOptions({ gfm: true, breaks: false });

@Component({
  selector: 'ml-assistant',
  template: `
    <header>
      <h2>Assistant</h2>
      <button class="ghost model" (click)="store.showSettings.set(true)" [title]="'Change in Settings'">{{ modelLabel() }}</button>
      @if (store.chat().length) {
        <button class="ghost" (click)="store.clearChat()" [disabled]="store.chatBusy()">Clear</button>
      }
      <button class="ghost" (click)="store.chatOpen.set(false)" aria-label="Close assistant">Close</button>
    </header>

    <div class="log" #log>
      @if (!store.chat().length) {
        <div class="intro">
          <p>Ask about any version of this song. The assistant reads MixLens's measurements, never your audio, and runs extra checks when it needs them.</p>
          @if (needsSetup()) {
            <div class="setup">
              <p>Connect a provider to start: an Anthropic or OpenAI API key, or Ollama running on this computer.</p>
              <button class="primary" (click)="store.showSettings.set(true)">Set up the assistant</button>
            </div>
          }
          <div class="suggest">
            @for (s of suggestions(); track s) {
              <button (click)="ask(s)" [disabled]="needsSetup() || !store.versions().length || store.chatBusy()">{{ s }}</button>
            }
          </div>
        </div>
      }
      @for (m of store.chat(); track m.id) {
        <div class="msg" [class.user]="m.role === 'user'">
          @if (m.role === 'user') { <p>{{ m.content }}</p> }
          @else { <div class="md" [innerHTML]="html().get(m.id)"></div> }
        </div>
      }
      @if (store.chatBusy()) {
        <div class="activity" aria-live="polite"><span class="pulse"></span>{{ store.activity() ?? 'Thinking' }}…</div>
      }
    </div>

    <form (submit)="submit($event)">
      <textarea #box rows="3" aria-label="Question for the assistant" [value]="draft()" (input)="draft.set(box.value)" (keydown)="key($event)"
        [disabled]="!store.projectId() || needsSetup()" [placeholder]="needsSetup() ? 'Set up the assistant to ask questions' : 'Ask about this mix or master'"></textarea>
      <button class="primary" type="submit" [disabled]="!draft().trim() || store.chatBusy() || !store.projectId()">Send</button>
    </form>
  `,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; }
    header { display: flex; align-items: center; gap: 6px; padding: 14px 16px 10px; border-bottom: 1px solid var(--line); }
    header h2 { margin-right: auto; }
    .model { font-size: var(--text-caption); max-width: 170px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .log { flex: 1; overflow-y: auto; padding: 14px 16px; display: flex; flex-direction: column; gap: 14px; }
    .intro p { color: var(--muted); margin-bottom: var(--space-s); }
    .setup { display: flex; flex-direction: column; align-items: flex-start; gap: var(--space-xs); margin-bottom: var(--space-m); }
    .setup p { color: var(--ink) !important; margin: 0 !important; }
    .suggest { display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
    .suggest button { text-align: left; background: transparent; }
    .msg { max-width: 100%; }
    .msg.user { align-self: flex-end; background: var(--raised); border: 1px solid var(--line); border-radius: 10px 10px 2px 10px; padding: var(--space-xs) var(--space-s); max-width: 88%; white-space: pre-wrap; }
    .activity { color: var(--muted); font-size: var(--text-body); display: flex; align-items: center; gap: var(--space-xs); }
    .pulse { width: 8px; height: 8px; border-radius: 50%; background: var(--a); animation: pulse 1.1s ease-in-out infinite; }
    @keyframes pulse { 50% { opacity: 0.25; } }
    form { display: flex; flex-direction: column; gap: var(--space-xs); padding: var(--space-s) var(--space-m) var(--space-m); border-top: 1px solid var(--line); }
    textarea { resize: none; width: 100%; }
    form button { align-self: flex-end; }
  `,
})
export class Assistant implements AfterViewChecked {
  readonly store = inject(Store);
  readonly draft = signal('');
  private readonly log = viewChild<ElementRef<HTMLElement>>('log');
  private lastCount = 0;

  readonly needsSetup = computed(() => !!this.store.settings() && !this.store.assistantReady());

  readonly modelLabel = computed(() => {
    const s = this.store.settings();
    if (!s) return '';
    if (this.needsSetup()) return 'Not set up';
    const m = s.provider === 'anthropic' ? s.anthropic_model : s.provider === 'openai' ? s.openai_model : s.ollama_model;
    return m || 'Choose a model';
  });

  readonly suggestions = computed(() => {
    const v = this.store.version();
    const many = this.store.versions().length > 1;
    const out = [];
    if (v?.kind === 'master') out.push('Is this master ready for Spotify and Apple Music?');
    else out.push('Is this mix ready to be mastered?');
    if (many) out.push('What changed since the previous version?');
    out.push('What should I fix first?');
    return out;
  });

  /** Parsed answers by their markdown, so each answer is parsed once rather than on every keystroke. */
  private readonly parsed = new Map<string, string>();

  /** HTML for each assistant message, by message id. Recomputed only when the chat changes. */
  readonly html = computed(() => {
    const out = new Map<number, string>();
    const keep = new Map<string, string>();
    for (const m of this.store.chat()) {
      if (m.role === 'user') continue;
      const h = this.parsed.get(m.content) ?? (marked.parse(m.content, { async: false }) as string);
      keep.set(m.content, h);
      out.set(m.id, h);
    }
    // Drop answers that are no longer in the chat (cleared, or another song).
    this.parsed.clear();
    keep.forEach((h, md) => this.parsed.set(md, h));
    return out;
  });

  ask(text: string) {
    void this.store.send(text);
  }

  submit(ev: Event) {
    ev.preventDefault();
    const t = this.draft();
    // Enter reaches here even while Send is disabled; keep the draft until it can be sent.
    if (!t.trim() || this.store.chatBusy() || !this.store.projectId()) return;
    this.draft.set('');
    void this.store.send(t);
  }

  key(ev: KeyboardEvent) {
    if (ev.key === 'Enter' && !ev.shiftKey && !ev.isComposing) this.submit(ev);
  }

  ngAfterViewChecked() {
    const n = this.store.chat().length + (this.store.chatBusy() ? 1 : 0);
    if (n !== this.lastCount) {
      this.lastCount = n;
      const el = this.log()?.nativeElement;
      if (el) el.scrollTop = el.scrollHeight;
    }
  }
}
