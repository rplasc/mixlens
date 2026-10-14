import { Component, inject, signal } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { Autofocus } from './dom';
import { keyName, num } from './fmt';
import { Sev } from './severity';
import { Store } from './store';

@Component({
  selector: 'ml-sidebar',
  imports: [NgTemplateOutlet, Autofocus, Sev],
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
          <input #nameBox aria-label="Song name" placeholder="Song name" (keydown.escape)="creating.set(false)" mlAutofocus />
        </form>
      }
      <ul class="projects">
        @for (p of store.projects(); track p.id) {
          <li class="project">
            <button class="item" [class.on]="p.id === store.projectId()" (click)="select(p.id)">
              <span class="name">{{ p.name }}</span>
              <span class="count">{{ p.version_count }}</span>
            </button>
            @if (p.id === store.projectId() && !confirmDelete()) {
              <button class="ghost small del" (click)="confirmDelete.set(true)" [attr.aria-label]="'Delete ' + p.name">Delete</button>
            }
          </li>
          @if (p.id === store.projectId() && confirmDelete()) {
            <li class="confirm" role="alert" (keydown.escape)="confirmDelete.set(false)">
              <p>Delete <strong>{{ p.name }}</strong>, its {{ p.version_count }} {{ p.version_count === 1 ? 'version' : 'versions' }} and assistant chat? Your audio files stay on disk.</p>
              <div class="row">
                <button class="danger small" (click)="remove()" [disabled]="deleting()">{{ deleting() ? 'Deleting…' : 'Delete song' }}</button>
                <button class="ghost small" (click)="confirmDelete.set(false)">Keep</button>
              </div>
            </li>
          }
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
            <li class="vrow">
              <button class="item version" [class.on]="v.id === store.versionId()" (click)="selectVersion(v.id)">
                <ml-sev class="status" [sev]="v.worst" [announce]="false" [attr.title]="issues(v)" />
                <span class="label">{{ v.label }}</span>
                <span class="kind">{{ v.kind }}</span>
                <span class="meta">{{ num(v.integrated_lufs) }} LUFS, {{ num(v.true_peak_dbtp) }} dBTP</span>
                <span class="sr-only">, {{ issues(v) }}</span>
              </button>
              <ng-container *ngTemplateOutlet="removeButton; context: { $implicit: v }" />
            </li>
            <ng-container *ngTemplateOutlet="removeConfirm; context: { $implicit: v }" />
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
            <li class="vrow">
              <button class="item version" [class.on]="v.id === store.versionId()" (click)="selectVersion(v.id)">
                <span class="status ref" aria-hidden="true"></span>
                <span class="label">{{ v.label }}</span>
                <span class="meta">{{ num(v.integrated_lufs) }} LUFS{{ v.key ? ', ' + keyName(v.key) : '' }}</span>
              </button>
              <ng-container *ngTemplateOutlet="removeButton; context: { $implicit: v }" />
            </li>
            <ng-container *ngTemplateOutlet="removeConfirm; context: { $implicit: v }" />
          } @empty {
            <li class="muted empty">Add a finished track you want this song to sit alongside. MixLens uses it for comparisons and doesn't check it for problems.</li>
          }
        </ul>
      </section>
    }

    <button class="ghost settings" (click)="store.showSettings.set(true)">Settings</button>

    <ng-template #removeButton let-v>
      @if (v.id === store.versionId() && removing() !== v.id) {
        <button class="ghost small del" (click)="removing.set(v.id)" [attr.aria-label]="'Remove ' + v.label">Remove</button>
      }
    </ng-template>
    <ng-template #removeConfirm let-v>
      @if (removing() === v.id) {
        <li class="confirm" role="alert" (keydown.escape)="removing.set(null)">
          <p>Remove <strong>{{ v.label }}</strong> and its analysis? The audio file stays on disk.</p>
          <div class="row">
            <button class="danger small" (click)="removeVersion()">Remove version</button>
            <button class="ghost small" (click)="removing.set(null)">Keep</button>
          </div>
        </li>
      }
    </ng-template>
  `,
  styles: `
    :host { display: flex; flex-direction: column; height: 100%; min-height: 0; padding: 14px 10px 10px; gap: 18px; }
    .brand { display: flex; align-items: center; gap: var(--space-xs); padding: 0 6px; font-family: var(--font-display); font-weight: 700; font-size: var(--text-subhead); }
    .head { display: flex; justify-content: space-between; align-items: center; padding: 0 6px 6px; }
    .small { font-size: var(--text-caption); padding: 2px 8px; }
    ul { list-style: none; margin: 0; padding: 0; }
    form { padding: 0 6px 6px; }
    form input { width: 100%; }
    .item { display: flex; width: 100%; text-align: left; background: transparent; border: 1px solid transparent; border-radius: var(--radius-s); padding: 6px 8px; gap: var(--space-xs); align-items: baseline; }
    .item:hover { background: var(--panel); border-color: transparent; }
    .item.on { background: var(--raised); border-color: var(--line); }
    .project { display: flex; align-items: center; gap: 2px; }
    .project .item { flex: 1; min-width: 0; }
    .del { color: var(--faint); opacity: 0; }
    .del:hover { color: var(--err); }
    /* Destructive actions stay out of sight until the row is hovered or reached by keyboard. */
    .project:hover .del, .vrow:hover .del, .del:focus-visible { opacity: 1; }
    .vrow { position: relative; }
    .vrow .del { position: absolute; right: 4px; bottom: 4px; }
    /* No hover on touch screens: keep the actions visible. */
    @media (hover: none) { .del { opacity: 1; } }
    .confirm { margin: var(--space-2xs) 0 var(--space-xs); padding: 8px 10px; border: 1px solid var(--err); border-radius: var(--radius-s); background: var(--panel); font-size: var(--text-caption); }
    .confirm p { color: var(--muted); margin-bottom: var(--space-xs); }
    .confirm .row { display: flex; gap: 6px; }
    .danger { border-color: var(--err); color: var(--err); background: transparent; }
    .projects .name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .count { color: var(--faint); font-size: var(--text-caption); }
    .versions { flex: 1; min-height: 0; overflow-y: auto; }
    .version { display: grid; grid-template-columns: 10px 1fr auto; grid-template-rows: auto auto; column-gap: var(--space-xs); row-gap: 1px; align-items: center; }
    .status { grid-row: 1; }
    .item.on .meta, .item.on .count { color: var(--muted); }
    .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .kind { font-size: var(--text-caption); color: var(--muted); border: 1px solid var(--line); border-radius: 3px; padding: 0 5px; }
    .meta { grid-column: 2 / 4; font-size: var(--text-caption); color: var(--faint); }
    .refs-head { margin-top: 14px; }
    .status.ref { width: 8px; height: 8px; border-radius: 50%; border: 1.5px solid var(--b); }
    .analyzing { display: flex; align-items: center; gap: var(--space-xs); padding: 6px 8px; color: var(--muted); font-size: var(--text-body); }
    .spin { width: 10px; height: 10px; border: 2px solid var(--line); border-top-color: var(--a); border-radius: 50%; animation: spin 0.8s linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    .empty { padding: var(--space-2xs) var(--space-xs); font-size: var(--text-caption); }
    .settings { margin-top: auto; align-self: flex-start; }
  `,
})
export class Sidebar {
  readonly store = inject(Store);
  readonly creating = signal(false);
  readonly confirmDelete = signal(false);
  readonly deleting = signal(false);
  /** Version whose removal is being confirmed. */
  readonly removing = signal<number | null>(null);
  readonly num = num;
  readonly keyName = keyName;

  select(id: number) {
    this.confirmDelete.set(false);
    this.store.navOpen.set(false);
    void this.store.selectProject(id);
  }

  issues(v: { errors: number; warnings: number }) {
    if (!v.errors && !v.warnings) return 'no problems found';
    return [v.errors && `${v.errors} to fix`, v.warnings && `${v.warnings} to check`].filter(Boolean).join(', ');
  }

  selectVersion(id: number) {
    this.removing.set(null);
    this.store.navOpen.set(false);
    void this.store.selectVersion(id);
  }

  async removeVersion() {
    this.removing.set(null);
    await this.store.deleteVersion();
  }

  async remove() {
    this.deleting.set(true);
    try {
      await this.store.deleteProject();
    } finally {
      this.deleting.set(false);
      this.confirmDelete.set(false);
    }
  }

  async create(ev: Event, name: string) {
    ev.preventDefault();
    this.creating.set(false);
    await this.store.createProject(name);
  }
}
