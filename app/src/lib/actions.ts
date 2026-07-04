// Shared Svelte actions.

/** Auto-growing textarea (industry pattern: grows with content up to a cap,
 *  then scrolls — Linear/Slack/ZCode composers behave this way). */
export function autogrow(el: HTMLTextAreaElement, maxPx = 220) {
  const fit = () => {
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, maxPx)}px`;
    el.style.overflowY = el.scrollHeight > maxPx ? "auto" : "hidden";
  };
  el.style.resize = "none";
  fit();
  el.addEventListener("input", fit);
  return {
    destroy() {
      el.removeEventListener("input", fit);
    },
  };
}
