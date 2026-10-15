export type Theme = "dark" | "light" | "system";

const KEY = "rscaps:theme";

function prefersLight(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ?? false;
}

/** The persisted mode (light | dark | system). */
export function getMode(): Theme {
  try {
    const v = localStorage.getItem(KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    /* ignore */
  }
  return "dark";
}

/** The resolved theme actually painted (light | dark). */
export function getTheme(): "dark" | "light" {
  const mode = getMode();
  if (mode === "system") return prefersLight() ? "light" : "dark";
  return mode;
}

export function setMode(mode: Theme): void {
  document.documentElement.dataset.theme =
    mode === "system" ? (prefersLight() ? "light" : "dark") : mode;
  try {
    localStorage.setItem(KEY, mode);
  } catch {
    /* ignore */
  }
}

/** Cycle light → dark → system (tscaps ThemeController.cycle). */
export function cycleTheme(): Theme {
  const order: Theme[] = ["light", "dark", "system"];
  const next = order[(order.indexOf(getMode()) + 1) % order.length];
  setMode(next);
  return next;
}

// Keep `system` mode live when the OS theme changes.
window.matchMedia?.("(prefers-color-scheme: light)").addEventListener("change", () => {
  if (getMode() === "system") setMode("system");
});
