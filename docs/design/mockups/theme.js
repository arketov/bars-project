// Тема окна: настройка «Оформление» — system | light | dark.
// В окне bars значение живёт в настройках; в мокапе — в localStorage.
(() => {
  "use strict";
  const media = matchMedia("(prefers-color-scheme: dark)");
  const get = () => { try { return localStorage.getItem("bars-theme") || "system"; } catch { return "system"; } };
  const apply = () => {
    const t = get();
    document.documentElement.dataset.mode = t === "system" ? (media.matches ? "dark" : "light") : t;
    document.querySelectorAll("[data-theme-set]").forEach((b) => b.classList.toggle("on", b.dataset.themeSet === t));
  };
  window.barsTheme = {
    get,
    set(t) { try { localStorage.setItem("bars-theme", t); } catch {} apply(); },
    toggle() { this.set(document.documentElement.dataset.mode === "dark" ? "light" : "dark"); },
  };
  media.addEventListener("change", apply);
  apply();
  addEventListener("DOMContentLoaded", apply);
})();
