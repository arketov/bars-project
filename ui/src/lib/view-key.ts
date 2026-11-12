// Вид проекта в одной строке (worktrees.md, «Окно»): `проект` — основной checkout, `проект␀wt:имя` — worktree.
// Ключ служит «проектом» буферов редактора и вкладок документа; команды окна получают его разобранным
// (`project` и `wt`). Модуль чистый: тесты, подменяющие `./api`, его не затрагивают.

const VIEW_SEPARATOR = "\u0000wt:";

export const viewKey = (project: string, wt: string | null | undefined): string => (wt ? `${project}${VIEW_SEPARATOR}${wt}` : project);

export function splitView(key: string): { project: string; wt: string | null } {
  const at = key.indexOf(VIEW_SEPARATOR);
  return at < 0 ? { project: key, wt: null } : { project: key.slice(0, at), wt: key.slice(at + VIEW_SEPARATOR.length) };
}
