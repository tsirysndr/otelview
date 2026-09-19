export type Theme = "dark" | "light";

export function applyTheme(theme: Theme) {
  const root = document.documentElement;
  root.classList.remove("dark", "light");
  root.classList.add(theme);
}

export function loadTheme(): Theme {
  return (localStorage.getItem("otelview.theme") as Theme) || "dark";
}

export function saveTheme(theme: Theme) {
  localStorage.setItem("otelview.theme", theme);
}
