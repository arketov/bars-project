// Цвета и иконки (Т-38): наборы и разрешение значений по пути.
// Устройство — docs/features/knowledge-base/colors-icons.md. Ключи — пути от корня документации
// с прямыми слешами; у документа с `.md`, у корневых файлов без `./`.

/** Палитра окна: значения под тему — токены `--pal-<имя>` в theme.css. Порядок — как в сетке выбора. */
export const COLORS = ["red", "orange", "amber", "green", "teal", "blue", "violet", "pink", "gray"] as const;
export type ColorId = (typeof COLORS)[number];

/** Встроенные иконки папок; рисует FolderIcon.svelte. `folder` — по умолчанию. */
export const ICONS = [
  "folder", "book", "gear", "code", "flask", "chat", "star", "bolt", "shield", "chart",
  "database", "globe", "users", "bug", "target", "rocket", "puzzle", "archive", "pencil", "magnifier",
] as const;
export type IconId = (typeof ICONS)[number];

export const COLOR_TITLE: Record<ColorId, string> = {
  red: "Красный", orange: "Оранжевый", amber: "Янтарный", green: "Зелёный", teal: "Бирюзовый",
  blue: "Синий", violet: "Фиолетовый", pink: "Розовый", gray: "Серый",
};

export const ICON_TITLE: Record<IconId, string> = {
  folder: "Папка", book: "Книга", gear: "Шестерня", code: "Код", flask: "Колба", chat: "Беседа", star: "Звезда",
  bolt: "Молния", shield: "Щит", chart: "График", database: "База данных", globe: "Глобус", users: "Люди",
  bug: "Ошибка", target: "Цель", rocket: "Ракета", puzzle: "Пазл", archive: "Архив", pencil: "Карандаш", magnifier: "Лупа",
};

/** Модуль — первый уровень от корня документации, подпапка — второй; глубже выбора нет. */
export const MAX_FOLDER_DEPTH = 2;

export interface Entry {
  color: string | null;
  icon: string | null;
}

export interface AppearanceData {
  folders: Record<string, Entry>;
  documents: Record<string, Entry>;
}

export const emptyAppearance = (): AppearanceData => ({ folders: {}, documents: {} });

export const validColor = (value: string | null | undefined): ColorId | null =>
  (COLORS as readonly string[]).includes(value ?? "") ? (value as ColorId) : null;

export const validIcon = (value: string | null | undefined): IconId | null =>
  (ICONS as readonly string[]).includes(value ?? "") ? (value as IconId) : null;

/** Ключ из пути узла дерева: корневые файлы приходят как `./имя`. */
export const keyOf = (treePath: string): string => treePath.replace(/^\.\//, "");

const parentOf = (key: string): string => (key.includes("/") ? key.slice(0, key.lastIndexOf("/")) : "");

/** Можно ли выбирать цвет и иконку папки: модуль или его подпапка, не корень и не глубже. */
export function canStyleFolder(key: string): boolean {
  return key !== "." && key !== "" && key.split("/").length <= MAX_FOLDER_DEPTH;
}

/** Цвет папки: свой, иначе ближайшей папки выше с цветом. */
export function folderColor(data: AppearanceData, key: string): ColorId | null {
  for (let folder = key; folder && folder !== "."; folder = parentOf(folder)) {
    const own = canStyleFolder(folder) ? validColor(data.folders[folder]?.color) : null;
    if (own) return own;
  }
  return null;
}

/** Иконка папки: только у модуля и подпапки второго уровня, не наследуется. */
export function folderIcon(data: AppearanceData, key: string): IconId | null {
  return canStyleFolder(key) ? validIcon(data.folders[key]?.icon) : null;
}

/** Цвет документа: свой, иначе цвет ближайшей папки выше. */
export function documentColor(data: AppearanceData, key: string): ColorId | null {
  return validColor(data.documents[key]?.color) ?? folderColor(data, parentOf(key));
}

/** Ключи записей, которых нет в списке документов дерева: такие записи не показываются. */
export function liveKeys(documents: Iterable<string>): Set<string> {
  const keys = new Set<string>();
  for (const path of documents) {
    keys.add(path);
    for (let folder = parentOf(path); folder; folder = parentOf(folder)) keys.add(folder);
  }
  return keys;
}

/** Значение CSS для цвета: токен темы. */
export const colorVar = (id: ColorId | null): string | undefined => (id ? `var(--pal-${id})` : undefined);
