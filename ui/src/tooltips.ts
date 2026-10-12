/** League's system tooltip presentation with pointer and keyboard access. */
export function wireTooltips(): void {
  let owner: HTMLElement | null = null;
  let tooltip: HTMLDivElement | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const id = "client-tooltip";

  const hide = () => {
    clearTimeout(timer);
    if (owner) {
      const descriptions = (owner.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(value => value && value !== id);
      if (descriptions.length) owner.setAttribute("aria-describedby", descriptions.join(" "));
      else owner.removeAttribute("aria-describedby");
    }
    tooltip?.remove();
    tooltip = null;
    owner = null;
  };
  const show = (target: HTMLElement) => {
    if (!target.isConnected || !target.getClientRects().length || !target.dataset.tooltip) return;
    tooltip = document.createElement("div");
    tooltip.id = id;
    tooltip.className = "client-tooltip";
    tooltip.setAttribute("role", "tooltip");
    // Account descriptions are untrusted plain text, never tooltip markup.
    tooltip.textContent = target.dataset.tooltip;
    document.body.append(tooltip);
    const anchor = target.getBoundingClientRect();
    const bounds = tooltip.getBoundingClientRect();
    const left = Math.max(8, Math.min(anchor.left + anchor.width / 2 - bounds.width / 2, innerWidth - bounds.width - 8));
    const above = anchor.top >= bounds.height + 20;
    tooltip.dataset.position = above ? "top" : "bottom";
    tooltip.style.left = `${left}px`;
    tooltip.style.top = `${Math.max(8, Math.min(above ? anchor.top - bounds.height - 12 : anchor.bottom + 12, innerHeight - bounds.height - 8))}px`;
    tooltip.style.setProperty("--caret-x", `${Math.max(12, Math.min(anchor.left + anchor.width / 2 - left, bounds.width - 12))}px`);
    target.setAttribute("aria-describedby", [target.getAttribute("aria-describedby"), id].filter(Boolean).join(" "));
  };
  const enter = (event: Event) => {
    const target = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-tooltip]") : null;
    if (!target || target === owner) return;
    hide();
    owner = target;
    timer = setTimeout(() => show(target), event.type === "focusin" ? 100 : 450);
  };
  const exit = (event: FocusEvent | PointerEvent) => {
    if (owner && !owner.contains(event.relatedTarget as Node | null)) hide();
  };
  document.addEventListener("pointerover", enter);
  document.addEventListener("focusin", enter);
  document.addEventListener("pointerout", exit);
  document.addEventListener("focusout", exit);
  document.addEventListener("pointerdown", hide, true);
  document.addEventListener("keydown", hide, true);
  document.addEventListener("scroll", hide, true);
  document.addEventListener("visibilitychange", hide);
  window.addEventListener("resize", hide);
  window.addEventListener("blur", hide);
  new MutationObserver(() => { if (owner && !owner.isConnected) hide(); })
    .observe(document.body, { childList: true, subtree: true });
}
