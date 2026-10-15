// Общее для мокапов состояний: значки, боковая панель, переключатель сцен.
(() => {
  "use strict";
  const s = (d, w = 16) => `<svg width="${w}" height="${w}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;
  window.I = {
    plus: s('<path d="M12 5v14M5 12h14"/>', 14),
    moon: s('<path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/>', 17),
    gear: s('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>', 17),
    warn: s('<path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z"/><path d="M12 9v4M12 17h.01"/>', 18),
    err: s('<circle cx="12" cy="12" r="10"/><path d="M15 9l-6 6M9 9l6 6"/>', 18),
    info: s('<circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>', 18),
    arrow: s('<path d="M5 12h14M13 6l6 6-6 6"/>', 18),
    folder: s('<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>', 30),
    search: s('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>', 30),
    file: s('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>', 30),
    plug: s('<path d="M9 2v6M15 2v6M6 8h12v4a6 6 0 0 1-12 0zM12 18v4"/>', 30),
    cloud: s('<path d="M18 10a6 6 0 0 0-11.5-1.5A4.5 4.5 0 0 0 7 18h11a4 4 0 0 0 0-8z"/><path d="m4 4 16 16"/>', 30),
    box: s('<path d="M21 8 12 3 3 8v8l9 5 9-5z"/><path d="m3 8 9 5 9-5M12 13v8"/>', 30),
  };

  // projects: [{name, sub, dot: ok|busy|warn|off|bad, on?}]
  window.sidebar = (projects, foot) => `
    <aside>
      <div class="brand"><div class="logo">b</div><div><b>bars</b><small>поиск по документации проектов</small></div></div>
      <div class="plist">
        <div class="plabel">Проекты</div>
        ${projects.map((p) => `<div class="pitem${p.on ? " on" : ""}"><span class="dot ${p.dot}"></span><span class="pn"><b>${p.name}</b><small>${p.sub}</small></span></div>`).join("") || '<div class="plabel" style="text-transform:none;font-weight:500;letter-spacing:0">Пока ни одного</div>'}
        <button class="padd">${I.plus}Добавить проект…</button>
      </div>
      <div class="sfoot">
        <div class="status">${foot}</div>
        <div style="display:flex;gap:2px">
          <button class="iconbtn" onclick="barsTheme.toggle()" title="Светлая / тёмная тема">${I.moon}</button>
          <a class="iconbtn" href="settings.html" title="Настройки">${I.gear}</a>
        </div>
      </div>
    </aside>`;

  // Кнопки .scenes [data-scene] показывают .scene#id; [data-side=id] — боковая панель сцены.
  window.initScenes = (first) => {
    const show = (id) => {
      document.querySelectorAll(".scene").forEach((e) => e.classList.toggle("on", e.id === id));
      document.querySelectorAll(".scenes [data-scene]").forEach((b) => b.classList.toggle("on", b.dataset.scene === id));
      try { history.replaceState(null, "", "#" + id); } catch {}
    };
    window.showScene = show;
    document.querySelectorAll(".scenes [data-scene]").forEach((b) => b.addEventListener("click", () => show(b.dataset.scene)));
    document.addEventListener("click", (e) => { const t = e.target.closest("[data-open]"); if (t) show(t.dataset.open); });
    const ids = [...document.querySelectorAll(".scene")].map((e) => e.id);
    const h = (location.hash || "").slice(1);
    show(ids.includes(h) ? h : first);
  };
})();
