/* Tier labels shrink to their card column instead of overflowing ("GRANDMASTER",
   German "HERAUSFORDERER"), down to a floor; long multi-word names wrap instead. */

const MIN_TIER_PX = 13;
// Below this, a multi-word name ("SEM CLASSIFICAÇÃO") reads better on two lines.
const WRAP_BELOW_PX = 16;

function fitTierNames(): void {
  document.querySelectorAll<HTMLElement>(".rank-name").forEach((name) => {
    name.style.removeProperty("font-size");
    name.classList.remove("is-wrapped");
    if (name.scrollWidth <= name.clientWidth) return;
    const base = Number.parseFloat(getComputedStyle(name).fontSize);
    const fitted = () => Math.max(MIN_TIER_PX, Math.floor(((base * name.clientWidth) / name.scrollWidth) * 10) / 10);
    let size = fitted();
    if (size < WRAP_BELOW_PX && name.textContent?.trim().includes(" ")) {
      // Wrapped, only the longest word has to fit.
      name.classList.add("is-wrapped");
      size = name.scrollWidth > name.clientWidth ? fitted() : base;
    }
    if (size < base) name.style.setProperty("font-size", `${size}px`);
  });
}

/** Refit whenever cards re-render or the collection is resized. */
export function watchTierNames(): void {
  let pending = 0;
  const schedule = () => {
    cancelAnimationFrame(pending);
    pending = requestAnimationFrame(fitTierNames);
  };
  const accounts = document.querySelector("#accounts");
  if (accounts) {
    new MutationObserver(schedule).observe(accounts, { childList: true, subtree: true, characterData: true });
    new ResizeObserver(schedule).observe(accounts);
  }
  void document.fonts.ready.then(schedule);
}
