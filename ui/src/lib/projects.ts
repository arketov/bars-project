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

// Группировка подпроектов в меню проектов (Т-49, Т-50; docs/features/projects/subprojects.md).

/** Поля проекта, нужные дереву переключателя. */
export interface SwitcherProject extends ProjectMatchFields {
  parent?: string | null;
  discovered?: boolean;
}

/** Одна видимая строка меню. `key` уникален для места строки: самостоятельный подпроект виден дважды. */
export interface SwitcherRow<T extends SwitcherProject = SwitcherProject> {
  key: string;
  project: T;
  depth: number;
  /** Верхний уровень — собственное имя; ниже — путь от непосредственного родителя. */
  label: string;
  hasChildren: boolean;
  expanded: boolean;
  /** Предок без совпадения с фильтром: приглушённая контекстная строка, остаётся выбираемой. */
  context: boolean;
  parentKey: string | null;
}

export interface SwitcherOptions {
  /** Текст фильтра; пустой — без фильтра. */
  filter?: string;
  /** Путь текущего проекта: его предки раскрыты по умолчанию. */
  current?: string | null;
  /** Ручные раскрытия и сворачивания по ключу пути (`pathKey`); они сильнее автораскрытия. */
  overrides?: ReadonlyMap<string, boolean>;
}

/** Ключ проекта в карте раскрытия: путь с `/` без хвостового разделителя, без учёта регистра. */
export function pathKey(path: string): string {
  return path.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
}

/** Верхний уровень: проекты без предка и самостоятельные подпроекты; найденные подпроекты — только под предком. */
export function isTopLevel(project: SwitcherProject): boolean {
  return !(project.discovered && project.parent);
}

export function topLevelProjects<T extends SwitcherProject>(projects: readonly T[]): T[] {
  return projects.filter(isTopLevel);
}

/** Путь подпроекта от его непосредственного родителя (`backend/api`); не вложен по пути — собственное имя. */
function relativeLabel(project: SwitcherProject, parent: SwitcherProject): string {
  const base = parent.path.replace(/\\/g, "/").replace(/\/+$/, "");
  const full = project.path.replace(/\\/g, "/").replace(/\/+$/, "");
  return full.toLowerCase().startsWith(`${base.toLowerCase()}/`) ? full.slice(base.length + 1) : project.name;
}

/** Плоский список видимых строк: верхний уровень в порядке реестра, подпроекты под предком по пути. */
export function buildSwitcher<T extends SwitcherProject>(projects: readonly T[], options: SwitcherOptions = {}): SwitcherRow<T>[] {
  const overrides = options.overrides ?? new Map<string, boolean>();
  const needle = (options.filter ?? "").trim();
  const filtering = needle !== "";
  const byPath = new Map<string, T>();
  for (const project of projects) byPath.set(pathKey(project.path), project);
  const children = new Map<string, T[]>();
  for (const project of projects) {
    const parent = project.parent ? pathKey(project.parent) : "";
    if (!parent || !byPath.has(parent)) continue;
    children.set(parent, [...(children.get(parent) ?? []), project]);
  }
  for (const list of children.values()) list.sort((a, b) => (pathKey(a.path) < pathKey(b.path) ? -1 : pathKey(a.path) > pathKey(b.path) ? 1 : 0));

  const currentAncestors = new Set<string>();
  let walker = options.current ? byPath.get(pathKey(options.current)) : undefined;
  while (walker?.parent) {
    const up = pathKey(walker.parent);
    if (currentAncestors.has(up) || !byPath.has(up)) break;
    currentAncestors.add(up);
    walker = byPath.get(up);
  }

  const build = (project: T, parent: T | null, parentKey: string | null, trail: ReadonlySet<string>): SwitcherRow<T>[] => {
    const id = pathKey(project.path);
    const key = `${parentKey ?? ""}>${id}`;
    const below = new Set(trail).add(id);
    const childRows = (children.get(id) ?? [])
      .filter((child) => !below.has(pathKey(child.path)))
      .flatMap((child) => build(child, project, key, below));
    const self = !filtering || matchProject(project, needle);
    if (!self && childRows.length === 0) return [];
    const hasChildren = childRows.length > 0;
    const auto = filtering ? hasChildren : currentAncestors.has(id);
    const expanded = hasChildren && (overrides.get(id) ?? auto);
    const row: SwitcherRow<T> = {
      key,
      project,
      depth: trail.size,
      label: parent ? relativeLabel(project, parent) : project.name,
      hasChildren,
      expanded,
      context: !self,
      parentKey,
    };
    return expanded ? [row, ...childRows] : [row];
  };

  return projects.filter(isTopLevel).flatMap((project) => build(project, null, null, new Set()));
}
