export type Theme = "dark" | "light";

const KEY = "rscaps:theme";

export function getTheme(): Theme {
  return (document.documentElement.dataset.theme as Theme) || "dark";
}

export function setTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(KEY, theme);
  } catch {
    /* ignore */
  }
}

export function toggleTheme(): Theme {
  const next: Theme = getTheme() === "dark" ? "light" : "dark";
  setTheme(next);
  return next;
}
