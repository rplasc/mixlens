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
    </header>

    <div class="log" #log>
      @if (!store.chat().length) {
        <div class="intro">
          <p>Ask about any version in this project. Answers use the measurements here, and the assistant runs more checks when it needs them.</p>
          @if (needsSetup()) {
            <p class="setup">Add an API key or pick a local model in <button class="link" (click)="store.showSettings.set(true)">Settings</button> to start.</p>
          }
          <div class="suggest">
            @for (s of suggestions(); track s) {
              <button (click)="ask(s)" [disabled]="!store.versions().length || store.chatBusy()">{{ s }}</button>
            }
          </div>
        </div>
      }
      @for (m of store.chat(); track m.id) {
        <div class="msg" [class.user]="m.role === 'user'">
          @if (m.role === 'user') { <p>{{ m.content }}</p> }
          @else { <div class="md" [innerHTML]="render(m.content)"></div> }
        </div>
      }
      @if (store.chatBusy()) {
        <div class="activity" aria-live="polite"><span class="pulse"></span>{{ store.activity() ?? 'Thinking' }}…</div>
      }
    </div>

    <form (submit)="submit($event)">
      <textarea #box rows="3" [value]="draft()" (input)="draft.set(box.value)" (keydown)="key($event)"
        [disabled]="!store.projectId()" placeholder="Ask about this mix or master"></textarea>
      <button class="primary" type="submit" [disabled]="!draft().trim() || store.chatBusy() || !store.projectId()">Send</button>
    </form>
  `,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; }
    header { display: flex; align-items: center; gap: 6px; padding: 14px 16px 10px; border-bottom: 1px solid var(--line); }
    header h2 { margin-right: auto; }
    .model { font-size: 12px; max-width: 170px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .log { flex: 1; overflow-y: auto; padding: 14px 16px; display: flex; flex-direction: column; gap: 14px; }
    .intro p { color: var(--muted); margin-bottom: 12px; }
    .setup { color: var(--warn) !important; }
    .link { background: none; border: none; padding: 0; color: var(--a); text-decoration: underline; cursor: pointer; }
    .suggest { display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
    .suggest button { text-align: left; background: transparent; }
    .msg { max-width: 100%; }
    .msg.user { align-self: flex-end; background: var(--raised); border: 1px solid var(--line); border-radius: 10px 10px 2px 10px; padding: 8px 12px; max-width: 88%; white-space: pre-wrap; }
    .activity { color: var(--muted); font-size: 13px; display: flex; align-items: center; gap: 8px; }
    .pulse { width: 8px; height: 8px; border-radius: 50%; background: var(--a); animation: pulse 1.1s ease-in-out infinite; }
    @keyframes pulse { 50% { opacity: 0.25; } }
    form { display: flex; flex-direction: column; gap: 8px; padding: 12px 16px 16px; border-top: 1px solid var(--line); }
    textarea { resize: none; width: 100%; }
    form button { align-self: flex-end; }
  `,
})
export class Assistant implements AfterViewChecked {
  readonly store = inject(Store);
  readonly draft = signal('');
  private readonly log = viewChild<ElementRef<HTMLElement>>('log');
  private lastCount = 0;

  readonly needsSetup = computed(() => {
    const s = this.store.settings();
    if (!s) return false;
    if (s.provider === 'anthropic') return !s.has_anthropic_key;
    if (s.provider === 'openai') return !s.has_openai_key || !s.openai_model;
    return !s.ollama_model;
  });

  readonly modelLabel = computed(() => {
    const s = this.store.settings();
    if (!s) return '';
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

  render(md: string): string {
    return marked.parse(md, { async: false }) as string;
  }

  ask(text: string) {
    void this.store.send(text);
  }

  submit(ev: Event) {
    ev.preventDefault();
    const t = this.draft();
    if (!t.trim()) return;
    this.draft.set('');
    void this.store.send(t);
  }

  key(ev: KeyboardEvent) {
    if (ev.key === 'Enter' && !ev.shiftKey) this.submit(ev);
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
