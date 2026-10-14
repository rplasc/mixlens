import { Component, computed, inject, OnInit, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { api, errorText, Provider, Settings as S } from './api';
import { Store } from './store';

@Component({
  selector: 'ml-settings',
  imports: [FormsModule],
  template: `
    <div class="scrim" (click)="close()"></div>
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title">
      <header>
        <h1 id="settings-title">Settings</h1>
        <button class="ghost" (click)="close()">Close</button>
      </header>

      @if (form(); as f) {
        <section>
          <h2>Assistant</h2>
          <div class="seg" role="radiogroup" aria-label="AI provider">
            @for (p of providers; track p.id) {
              <button type="button" role="radio" [attr.aria-checked]="f.provider === p.id" [class.on]="f.provider === p.id"
                (click)="setProvider(p.id)">{{ p.label }}</button>
            }
          </div>
          <p class="hint">{{ providerHint() }}</p>

          @if (f.provider !== 'ollama') {
            <label class="field">
              <span>{{ f.provider === 'anthropic' ? 'Anthropic' : 'OpenAI' }} API key</span>
              <div class="row">
                <input type="password" [(ngModel)]="keyDraft" [placeholder]="hasKey() ? 'Saved in Windows Credential Manager' : 'Paste your key'" autocomplete="off" />
                <button (click)="saveKey()" [disabled]="!keyDraft.trim()">Save key</button>
                @if (hasKey()) { <button class="ghost" (click)="removeKey()">Remove</button> }
              </div>
            </label>
          } @else {
            <label class="field">
              <span>Ollama address</span>
              <input [(ngModel)]="f.ollama_url" (change)="persist()" />
            </label>
          }

          <label class="field">
            <span>Model</span>
            <div class="row">
              <input [ngModel]="model()" (ngModelChange)="setModel($event)" (change)="persist()" list="models" placeholder="Model name" />
              <datalist id="models">@for (m of models(); track m) { <option [value]="m"></option> }</datalist>
              <button (click)="loadModels()" [disabled]="loadingModels()">{{ loadingModels() ? 'Loading…' : 'Load models' }}</button>
            </div>
          </label>
          @if (modelsError()) { <p class="err">{{ modelsError() }}</p> }
        </section>

        <section>
          <h2>Analysis</h2>
          <label class="field">
            <span>Main streaming platform</span>
            <select [(ngModel)]="f.main_platform" (change)="persist()">
              @for (p of store.platforms(); track p.id) {
                <option [ngValue]="p.id">{{ p.name }} ({{ p.target_lufs }} LUFS{{ p.normalization === 'down_only' ? ', turns down only' : '' }})</option>
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
          <p class="hint">Masters are checked against this platform's loudness target and true-peak ceiling. The Streaming tab shows every platform.</p>
          <label class="field">
            <span>ffmpeg location, for the encode check</span>
            <input [(ngModel)]="f.ffmpeg_path" (change)="persist()" placeholder="Leave empty to use ffmpeg from PATH" />
          </label>
          <p class="hint" [class.ok]="store.settings()?.ffmpeg_found" [class.err]="!store.settings()?.ffmpeg_found">
            {{ store.settings()?.ffmpeg_found ? 'ffmpeg found.' : 'ffmpeg not found. Install it with "winget install Gyan.FFmpeg", restart MixLens, or set its full path here.' }}
          </p>
        </section>
      }
      @if (saved()) { <p class="saved" aria-live="polite">{{ saved() }}</p> }
    </div>
  `,
  styles: `
    :host { position: fixed; inset: 0; z-index: 20; display: grid; place-items: center; }
    .scrim { position: absolute; inset: 0; background: rgba(10, 13, 16, 0.6); }
    .dialog { position: relative; width: min(560px, 92vw); max-height: 88vh; overflow-y: auto; background: var(--panel);
      border: 1px solid var(--line); border-radius: var(--radius-m); padding: 20px 24px 22px; }
    header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; }
    section { padding: 14px 0; border-top: 1px solid var(--line-soft); }
    section h2 { margin-bottom: 10px; }
    .seg { display: inline-flex; border: 1px solid var(--line); border-radius: var(--radius-s); overflow: hidden; }
    .seg button { border: none; border-radius: 0; background: transparent; padding: 6px 14px; }
    .seg button + button { border-left: 1px solid var(--line); }
    .seg button.on { background: var(--a); color: #221c0c; font-weight: 600; }
    .field { display: flex; flex-direction: column; gap: 4px; margin-top: 12px; }
    .field > span { font-size: 12px; color: var(--muted); }
    .row { display: flex; gap: 8px; }
    .row input { flex: 1; min-width: 0; }
    .short { width: 120px; }
    .hint { font-size: 12px; color: var(--faint); margin-top: 8px; }
    .ok { color: var(--ok); }
    .err { color: var(--err); font-size: 12px; margin-top: 6px; }
    .saved { font-size: 12px; color: var(--ok); }
  `,
})
export class Settings implements OnInit {
  readonly store = inject(Store);
  readonly form = signal<S | null>(null);
  readonly models = signal<string[]>([]);
  readonly loadingModels = signal(false);
  readonly modelsError = signal<string | null>(null);
  readonly saved = signal<string | null>(null);
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
      case 'anthropic': return 'Uses your Anthropic API key. Only measurements are sent, never audio.';
      case 'openai': return 'Uses your OpenAI API key. Only measurements are sent, never audio.';
      default: return 'Runs on this computer with Ollama, so nothing leaves your machine. Pick a model that supports tool calling.';
    }
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

  async setProvider(p: Provider) {
    const f = this.form();
    if (!f) return;
    this.form.set({ ...f, provider: p });
    this.models.set([]);
    this.modelsError.set(null);
    this.keyDraft = '';
    await this.persist();
  }

  async persist() {
    const f = this.form();
    if (!f) return;
    try {
      this.store.settings.set(await api.saveSettings({ ...f, target_lufs: Number(f.target_lufs) }));
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
    this.store.settings.set(await api.setApiKey(f.provider, ''));
    this.flash('Key removed');
  }

  async loadModels() {
    const f = this.form();
    if (!f) return;
    this.loadingModels.set(true);
    this.modelsError.set(null);
    try {
      const list = await api.listModels(f.provider);
      this.models.set(list);
      if (!this.model() && list.length) {
        this.setModel(list[0]);
        await this.persist();
      }
    } catch (e) {
      this.modelsError.set(errorText(e));
    } finally {
      this.loadingModels.set(false);
    }
  }

  private flash(text: string) {
    this.saved.set(text);
    setTimeout(() => this.saved.set(null), 1600);
  }

  close() {
    this.store.showSettings.set(false);
  }
}
