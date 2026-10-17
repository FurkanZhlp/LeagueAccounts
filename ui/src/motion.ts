import { reducedMotion } from "./dom";

/** The client's short flat-button click scale, sheen and corner flare. */
export function wireControlMotion(): void {
  const timers = new WeakMap<HTMLElement, ReturnType<typeof setTimeout>>();
  document.addEventListener("click", event => {
    const button = event.target instanceof Element ? event.target.closest<HTMLElement>(".btn:not(:disabled)") : null;
    if (!button || reducedMotion()) return;
    clearTimeout(timers.get(button));
    button.classList.remove("is-client-click");
    void button.offsetWidth;
    button.classList.add("is-client-click");
    timers.set(button, setTimeout(() => button.classList.remove("is-client-click"), 430));
  }, true);
}
