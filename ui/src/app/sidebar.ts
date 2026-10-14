import { Component, inject, signal } from '@angular/core';
import { num } from './fmt';
import { Store } from './store';

@Component({
  selector: 'ml-sidebar',
  template: `
    <div class="brand">
      <svg width="22" height="22" viewBox="0 0 22 22" aria-hidden="true">
        <circle cx="11" cy="11" r="9" fill="none" stroke="var(--b)" stroke-width="2" />
        <path d="M6 11v0M8 8v6M10 6v10M12 9v4M14 7v8M16 10v2" stroke="var(--a)" stroke-width="1.6" stroke-linecap="round" />
      </svg>
      <span>MixLens</span>
    </div>

    <section>
      <div class="head">
        <h3>Songs</h3>
        <button class="ghost small" (click)="creating.set(true)">New</button>
      </div>
      @if (creating()) {
        <form (submit)="create($event, nameBox.value)">
          <input #nameBox placeholder="Song name" (keydown.escape)="creating.set(false)" autofocus />
        </form>
      }
      <ul class="projects">
        @for (p of store.projects(); track p.id) {
          <li>
            <button class="item" [class.on]="p.id === store.projectId()" (click)="store.selectProject(p.id)">
              <span class="name">{{ p.name }}</span>
              <span class="count">{{ p.version_count }}</span>
            </button>
          </li>
        } @empty {
          @if (!creating()) { <li class="muted empty">Drop audio files anywhere to start.</li> }
        }
      </ul>
    </section>

    @if (store.project()) {
      <section class="versions">
        <div class="head">
          <h3>Versions</h3>
          <button class="ghost small" (click)="store.pickFiles()">Add files</button>
        </div>
        <ul>
          @for (v of store.mixVersions(); track v.id) {
            <li>
              <button class="item version" [class.on]="v.id === store.versionId()" (click)="store.selectVersion(v.id)">
                <span class="status" [attr.data-sev]="v.worst" [attr.title]="v.errors + ' errors, ' + v.warnings + ' warnings'"></span>
                <span class="label">{{ v.label }}</span>
                <span class="kind">{{ v.kind }}</span>
                <span class="meta">{{ num(v.integrated_lufs) }} LUFS, {{ num(v.true_peak_dbtp) }} dBTP</span>
              </button>
            </li>
          }
          @for (a of store.analyzing(); track a) {
            <li class="analyzing"><span class="spin"></span>Analyzing {{ a }}</li>
          }
        </ul>
        @if (!store.mixVersions().length && !store.analyzing().length) {
          <p class="muted empty">Add a bounce of this song to analyze it. You can also drop files onto the window.</p>
        }

        <div class="head refs-head">
          <h3>References</h3>
          <button class="ghost small" (click)="store.pickFiles('reference')">Add reference</button>
        </div>
        <ul>
          @for (v of store.references(); track v.id) {
            <li>
              <button class="item version" [class.on]="v.id === store.versionId()" (click)="store.selectVersion(v.id)">
                <span class="status ref" aria-hidden="true"></span>
                <span class="label">{{ v.label }}</span>
                <span class="kind ref">ref</span>
                <span class="meta">{{ num(v.integrated_lufs) }} LUFS, {{ num(v.true_peak_dbtp) }} dBTP</span>
              </button>
            </li>
          } @empty {
            <li class="muted empty">A finished track you'd like this song to sit next to. Not checked for problems; used for comparisons.</li>
          }
        </ul>
      </section>
    }

    <button class="ghost settings" (click)="store.showSettings.set(true)">Settings</button>
  `,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; padding: 14px 10px 10px; gap: 18px; }
    .brand { display: flex; align-items: center; gap: 8px; padding: 0 6px; font-family: var(--font-display); font-weight: 700; font-size: 16px; }
    .head { display: flex; justify-content: space-between; align-items: center; padding: 0 6px 6px; }
    .small { font-size: 12px; padding: 2px 8px; }
    ul { list-style: none; margin: 0; padding: 0; }
    form { padding: 0 6px 6px; }
    form input { width: 100%; }
    .item { display: flex; width: 100%; text-align: left; background: transparent; border: 1px solid transparent; border-radius: var(--radius-s); padding: 6px 8px; gap: 8px; align-items: baseline; }
    .item:hover { background: var(--panel); border-color: transparent; }
    .item.on { background: var(--raised); border-color: var(--line); }
    .projects .name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .count { color: var(--faint); font-size: 12px; }
    .versions { flex: 1; min-height: 0; overflow-y: auto; }
    .version { display: grid; grid-template-columns: 10px 1fr auto; grid-template-rows: auto auto; column-gap: 8px; row-gap: 1px; align-items: center; }
    .status { width: 8px; height: 8px; border-radius: 50%; background: var(--ok); grid-row: 1; }
    .status[data-sev='warn'] { background: var(--warn); }
    .status[data-sev='error'] { background: var(--err); }
    .status[data-sev='info'] { background: var(--info); }
    .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .kind { font-size: 11px; color: var(--muted); border: 1px solid var(--line); border-radius: 3px; padding: 0 5px; }
    .meta { grid-column: 2 / 4; font-size: 12px; color: var(--faint); }
    .refs-head { margin-top: 14px; }
    .status.ref { background: transparent; border: 1.5px solid var(--b); }
    .kind.ref { color: var(--b); border-color: var(--b); }
    .analyzing { display: flex; align-items: center; gap: 8px; padding: 6px 8px; color: var(--muted); font-size: 13px; }
    .spin { width: 10px; height: 10px; border: 2px solid var(--line); border-top-color: var(--a); border-radius: 50%; animation: spin 0.8s linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    .empty { padding: 4px 8px; font-size: 13px; }
    .settings { margin-top: auto; align-self: flex-start; }
  `,
})
export class Sidebar {
  readonly store = inject(Store);
  readonly creating = signal(false);
  readonly num = num;

  async create(ev: Event, name: string) {
    ev.preventDefault();
    this.creating.set(false);
    await this.store.createProject(name);
  }
}
