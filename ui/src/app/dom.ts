import { AfterViewInit, DestroyRef, Directive, ElementRef, Signal, inject, signal } from '@angular/core';

/** The host element's width in pixels, kept current with a ResizeObserver. Call from a field initializer. */
export function hostWidth(initial: number, min: number): Signal<number> {
  const el: HTMLElement = inject(ElementRef).nativeElement;
  const w = signal(initial);
  const ro = new ResizeObserver((e) => w.set(Math.max(min, Math.floor(e[0].contentRect.width))));
  ro.observe(el);
  inject(DestroyRef).onDestroy(() => ro.disconnect());
  return w.asReadonly();
}

/** Focuses its element when it appears. The `autofocus` attribute is ignored once anything on the page has focus. */
@Directive({ selector: '[mlAutofocus]' })
export class Autofocus implements AfterViewInit {
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef);

  ngAfterViewInit() {
    this.el.nativeElement.focus();
  }
}
