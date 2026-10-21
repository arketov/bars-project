// Должен импортироваться раньше App: тема окна читается из localStorage при загрузке модуля `theme.svelte`.
// Параметр адреса theme=light|dark нужен только стенду, чтобы открывать нужную тему ссылкой.
const theme = new URLSearchParams(location.search).get("theme");
if (theme === "light" || theme === "dark") {
  try {
    localStorage.setItem("bars-theme", theme);
  } catch {
    // Без хранилища тема остаётся системной.
  }
}
