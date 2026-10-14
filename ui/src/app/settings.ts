import { AfterViewInit, Component, computed, ElementRef, inject, OnInit, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { api, errorText, Provider, Settings as S } from './api';
import { num } from './fmt';
import { Sev } from './severity';
import { Store } from './store';

@Component({
  selector: 'ml-settings',
  imports: [FormsModule, Sev],
  template: `
    <!-- Native modal dialog: traps focus, closes on Escape and returns focus when it closes. -->
    <dialog #dialog aria-labelledby="settings-title" (close)="store.showSettings.set(false)" (click)="backdropClick($event)">
     <div class="body">
      <header>
        <div class="title">
          <h1 id="settings-title">Settings</h1>
          <p class="lede">Changes save as you make them.</p>
        </div>
        <span class="saved" aria-live="polite">{{ saved() }}</span>
        <button class="ghost" (click)="close()">Done</button>
      </header>

      @if (form(); as f) {
        <section>
          <h2>Analysis</h2>
          <label class="field">
            <span>Main streaming platform</span>
            <select [(ngModel)]="f.main_platform" (change)="persist()">
              @for (p of store.platforms(); track p.id) {
                <option [ngValue]="p.id">{{ p.name }} ({{ num(p.target_lufs, 0) }} LUFS{{ p.normalization === 'down_only' ? ', turns down only' : '' }})</option>
              }
              <option ngValue="custom">Custom target</option>
            </select>
          </label>
          @if (f.main_platform === 'custom') {
            <label class="field">
              <span>Loudness target (LUFS)</span>
              <input type="number" step="0.5" [(ngModel)]="f.target_lufs" (change)="persist()" class="short" />
            </label>
          }
          <p class="hint">MixLens checks masters against this platform's loudness target and true-peak ceiling. The Streaming tab shows all of them.</p>
          <label class="field">
            <span>ffmpeg (used by the encode check)</span>
            <input [(ngModel)]="f.ffmpeg_path" (change)="persist()" placeholder="Leave empty to find ffmpeg automatically" />
          </label>
          <p class="hint" [class.ok]="store.settings()?.ffmpeg_found" [class.err]="!store.settings()?.ffmpeg_found">
            {{ store.settings()?.ffmpeg_found ? 'ffmpeg found. The encode check on the Streaming tab is ready.'
              : 'MixLens can\\'t find ffmpeg, so the encode check won\\'t run. Install it with "winget install Gyan.FFmpeg" and restart MixLens, or enter the full path to ffmpeg.exe above.' }}
          </p>
        </section>

        <section>
          <h2>Assistant</h2>
          @if (status(); as st) {
            <p class="status" role="status"><ml-sev [sev]="st.ok ? 'ok' : 'info'" [announce]="false" />{{ st.text }}</p>
          }
          <div class="seg" role="group" aria-label="AI provider">
            @for (p of providers; track p.id) {
              <button type="button" [attr.aria-pressed]="f.provider === p.id" [class.on]="f.provider === p.id"
                (click)="setProvider(p.id)">{{ p.label }}</button>
            }
          </div>
          <p class="hint">{{ providerHint() }}</p>

          @if (f.provider !== 'ollama') {
            <label class="field">
              <span>{{ f.provider === 'anthropic' ? 'Anthropic' : 'OpenAI' }} API key</span>
              <div class="row">
                <input #keyBox type="password" [(ngModel)]="keyDraft" [placeholder]="hasKey() ? 'Key saved' : 'Paste your key'" autocomplete="off" />
                <button [class.primary]="keyDraft.trim()" (click)="saveKey()" [disabled]="!keyDraft.trim()">Save key</button>
                @if (hasKey()) { <button class="ghost" (click)="removeKey()">Remove key</button> }
              </div>
            </label>
            @if (store.settings()?.keyring_ok) { <p class="hint">{{ keyHint() }}</p> }
          } @else {
            <label class="field">
              <span>Ollama address</span>
              <input [(ngModel)]="f.ollama_url" (change)="persist()" placeholder="http://localhost:11434" />
            </label>
          }

          <label class="field">
            <span>Model</span>
            <div class="row">
              @if (models().length && !typingModel()) {
                <select [ngModel]="model()" (ngModelChange)="pickModel($event)" aria-label="Model">
                  @if (model() && !models().includes(model())) { <option [ngValue]="model()">{{ model() }}</option> }
                  @for (m of models(); track m) { <option [ngValue]="m">{{ m }}</option> }
                  <option [ngValue]="OTHER">Type a model name…</option>
                </select>
              } @else {
                <input #modelBox [ngModel]="model()" (ngModelChange)="setModel($event)" (change)="persist()" placeholder="Model name" />
              }
              <button (click)="loadModels()" [disabled]="loadingModels() || (f.provider !== 'ollama' && !hasKey())">{{ loadingModels() ? 'Loading…' : 'Load models' }}</button>
            </div>
          </label>
          @if (modelsError()) { <p class="err">{{ modelsError() }}</p> } @else { <p class="hint">{{ modelHint() }}</p> }
        </section>
      }
     </div>
    </dialog>
  `,
  styles: `
    :host { display: contents; }
    dialog { width: min(560px, 92vw); max-height: 88vh; overflow-y: auto; padding: 0; color: var(--ink); background: var(--panel);
      border: 1px solid var(--line); border-radius: var(--radius-m); }
    dialog::backdrop { background: var(--scrim); }
    .body { padding: var(--space-l); }
    header { display: flex; align-items: center; gap: var(--space-s); margin-bottom: var(--space-xs); }
    header .title { margin-right: auto; }
    .lede { font-size: var(--text-caption); color: var(--faint); }
    .status { display: flex; align-items: center; gap: var(--space-xs); margin-bottom: var(--space-m); }
    header h1 { font-size: var(--text-title); }
    section { padding: var(--space-l) 0; border-top: 1px solid var(--line-soft); }
    section:last-child { padding-bottom: 0; }
    section h2 { margin-bottom: var(--space-s); }
    .field { display: flex; flex-direction: column; gap: var(--space-2xs); margin-top: var(--space-m); }
    h2 + .field { margin-top: 0; }
    .field > span { font-size: var(--text-caption); color: var(--muted); }
    .row { display: flex; gap: var(--space-xs); }
    .row input, .row select { flex: 1; min-width: 0; }
    .short { width: 120px; }
    .hint { font-size: var(--text-caption); color: var(--faint); margin-top: var(--space-2xs); max-width: 64ch; }
    .ok { color: var(--ok); }
    .err { color: var(--err); font-size: var(--text-caption); margin-top: var(--space-2xs); }
    .saved { font-size: var(--text-caption); color: var(--ok); }
  `,
})
export class Settings implements OnInit, AfterViewInit {
  readonly store = inject(Store);
  private readonly dialog = viewChild.required<ElementRef<HTMLDialogElement>>('dialog');
  private readonly keyBox = viewChild<ElementRef<HTMLInputElement>>('keyBox');
  private readonly modelBox = viewChild<ElementRef<HTMLInputElement>>('modelBox');
  /** Picker value that switches the model field to free text. */
  readonly OTHER = '\u0000other';
  /** The person chose to type a model name rather than pick from the loaded list. */
  readonly typingModel = signal(false);
  readonly form = signal<S | null>(null);
  readonly models = signal<string[]>([]);
  readonly loadingModels = signal(false);
  readonly modelsError = signal<string | null>(null);
  readonly saved = signal<string | null>(null);
  readonly num = num;
  keyDraft = '';

  readonly providers: { id: Provider; label: string }[] = [
    { id: 'anthropic', label: 'Anthropic' },
    { id: 'openai', label: 'OpenAI' },
    { id: 'ollama', label: 'Ollama (offline)' },
  ];

  ngOnInit() {
    const s = this.store.settings();
    if (s) {
      const { has_anthropic_key, has_openai_key, ffmpeg_found, keyring_ok, effective_target_lufs, ...rest } = s;
      this.form.set({ ...rest });
    }
  }

  readonly hasKey = computed(() => {
    const s = this.store.settings();
    const f = this.form();
    return f?.provider === 'anthropic' ? !!s?.has_anthropic_key : !!s?.has_openai_key;
  });

  readonly providerHint = computed(() => {
    switch (this.form()?.provider) {
      case 'anthropic': return 'Your questions and MixLens\'s measurements go to Anthropic. Your audio never leaves this computer.';
      case 'openai': return 'Your questions and MixLens\'s measurements go to OpenAI. Your audio never leaves this computer.';
      default: return 'Ollama runs the assistant on this computer, so nothing is sent anywhere.';
    }
  });

  /** Whether the assistant can answer yet, and the next step if it can't. */
  readonly status = computed(() => {
    const f = this.form();
    const s = this.store.settings();
    if (!f || !s) return null;
    const name = f.provider === 'anthropic' ? 'Anthropic' : f.provider === 'openai' ? 'OpenAI' : 'Ollama';
    if (this.store.assistantReady()) return { ok: true, text: `Ready. The assistant uses ${name} with ${this.model()}.` };
    if (f.provider === 'ollama') return { ok: false, text: 'Not set up yet. Start Ollama, then load its models and choose one.' };
    if (!s.keyring_ok) {
      return { ok: false, text: "Windows Credential Manager isn't available, so MixLens can't store an API key. Ollama runs the assistant without one." };
    }
    if (!this.hasKey()) return { ok: false, text: `Not set up yet. Paste ${f.provider === 'anthropic' ? 'an Anthropic' : 'an OpenAI'} API key below to start.` };
    return { ok: false, text: 'Almost ready. Load the models and choose one.' };
  });

  readonly keyHint = computed(() => {
    if (this.hasKey()) return 'A key is saved in Windows Credential Manager. Paste a new one to replace it.';
    const site = this.form()?.provider === 'openai' ? 'platform.openai.com' : 'console.anthropic.com';
    return `Create one at ${site}, under API keys. MixLens keeps it in Windows Credential Manager, not in its own files.`;
  });

  readonly modelHint = computed(() => {
    const f = this.form();
    if (!f) return '';
    const n = this.models().length;
    if (this.typingModel()) return 'Type the exact model name, as the provider lists it.';
    if (n) return `${n} ${n === 1 ? 'model' : 'models'} available for this ${f.provider === 'ollama' ? 'Ollama install' : 'key'}.`;
    if (f.provider !== 'ollama' && !this.hasKey()) return 'Save a key first, then load the models it can use.';
    if (f.provider === 'anthropic') return 'claude-sonnet-5-5 is the default. Load models to see the others your key can use.';
    if (f.provider === 'openai') return 'Load models to list the chat models your key can use.';
    return "Load models to list the ones you've pulled in Ollama. Choose one that supports tool calling.";
  });

  model() {
    const f = this.form();
    if (!f) return '';
    return f.provider === 'anthropic' ? f.anthropic_model : f.provider === 'openai' ? f.openai_model : f.ollama_model;
  }

  setModel(v: string) {
    const f = this.form();
    if (!f) return;
    const next = { ...f };
    if (f.provider === 'anthropic') next.anthropic_model = v;
    else if (f.provider === 'openai') next.openai_model = v;
    else next.ollama_model = v;
    this.form.set(next);
  }

  pickModel(v: string) {
    if (v === this.OTHER) {
      this.typingModel.set(true);
      setTimeout(() => this.modelBox()?.nativeElement.focus());
      return;
    }
    this.setModel(v);
    void this.persist();
  }

  async setProvider(p: Provider) {
    const f = this.form();
    if (!f) return;
    this.form.set({ ...f, provider: p });
    this.models.set([]);
    this.modelsError.set(null);
    this.typingModel.set(false);
    this.keyDraft = '';
    await this.persist();
  }

  async persist() {
    const f = this.form();
    if (!f) return;
    // A cleared number field reads as null, which Number() would turn into 0 LUFS: keep the saved target.
    const typed = f.target_lufs as number | string | null;
    const valid = typed != null && typed !== '' && isFinite(Number(typed));
    const target_lufs = valid ? Number(typed) : (this.store.settings()?.target_lufs ?? -14);
    try {
      this.store.settings.set(await api.saveSettings({ ...f, target_lufs }));
      this.flash('Saved');
    } catch (e) {
      this.store.fail(e);
    }
  }

  async saveKey() {
    const f = this.form();
    if (!f || f.provider === 'ollama') return;
    try {
      this.store.settings.set(await api.setApiKey(f.provider, this.keyDraft));
      this.keyDraft = '';
      this.flash('Key saved');
      await this.loadModels();
    } catch (e) {
      this.store.fail(e);
    }
  }

  async removeKey() {
    const f = this.form();
    if (!f || f.provider === 'ollama') return;
    try {
      this.store.settings.set(await api.setApiKey(f.provider, ''));
      this.flash('Key removed');
    } catch (e) {
      this.store.fail(e);
    }
  }

  async loadModels() {
    const f = this.form();
    if (!f) return;
    this.loadingModels.set(true);
    this.modelsError.set(null);
    try {
      const list = await api.listModels(f.provider);
      // The provider may have changed while the list loaded.
      if (this.form()?.provider !== f.provider) return;
      this.models.set(list);
      this.typingModel.set(false);
      if (!this.model() && list.length) {
        this.setModel(list[0]);
        await this.persist();
      }
    } catch (e) {
      if (this.form()?.provider !== f.provider) return;
      this.modelsError.set(
        f.provider === 'ollama' ? `Couldn't reach Ollama at ${f.ollama_url}. Start Ollama and try again.` : `Couldn't load models: ${errorText(e)}`,
      );
    } finally {
      this.loadingModels.set(false);
    }
  }

  private flashTimer?: ReturnType<typeof setTimeout>;
  private flash(text: string) {
    clearTimeout(this.flashTimer);
    this.saved.set(text);
    this.flashTimer = setTimeout(() => this.saved.set(null), 1600);
  }

  ngAfterViewInit() {
    this.dialog().nativeElement.showModal();
    // Most people arrive here to connect the assistant: start them in the key field.
    if (!this.store.assistantReady() && !this.hasKey()) this.keyBox()?.nativeElement.focus();
  }

  /** Closing fires the dialog's close event, which removes this component. */
  close() {
    this.dialog().nativeElement.close();
  }

  /** A click on the dialog element itself (not its content) is a click on the backdrop. */
  backdropClick(ev: MouseEvent) {
    if (ev.target === this.dialog().nativeElement) this.close();
  }
}
