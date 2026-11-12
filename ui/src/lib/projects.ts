// Список проектов в меню окна: сопоставление с фильтром (Т-50).
// Правило то же, что у `projects(filter)` воркера и `bars_status`: подстрока без учёта регистра
// по имени, составному имени и пути; `\` и `/` равны. Одна тестовая таблица на окно и воркер.

/** Поля проекта, нужные фильтру. `qualified_name` приходит от воркера с шага подпроектов; пока его нет — не учитывается. */
export interface ProjectMatchFields {
  name: string;
  path: string;
  qualified_name?: string | null;
}

function normalize(value: string): string {
  return value.replace(/\\/g, "/").toLowerCase();
}

/** Пустой (после обрезки пробелов) текст подходит всем проектам. */
export function matchProject(project: ProjectMatchFields, text: string): boolean {
  const needle = normalize(text.trim());
  if (needle === "") return true;
  return [project.name, project.qualified_name, project.path].some(
    (field) => typeof field === "string" && normalize(field).includes(needle),
  );
}
