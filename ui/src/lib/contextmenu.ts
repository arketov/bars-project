/** Логика контекстного меню окна (ISS-07): какие пункты показать и как их выполнить. */

export type MenuAction = "copy" | "paste" | "selectAll" | "open" | "editors" | "editor" | "reveal" | "path" | "check" | "deleteProject" | "checkProject";

export interface MenuItem {
  action: MenuAction;
  label: string;
  editor?: string;
  disabled?: boolean;
  hint?: string;
}

export function projectAt(target: EventTarget | null): string | null {
  const element = target instanceof Element ? target.closest<HTMLElement>("[data-project-path]") : null;
  return element?.dataset.projectPath || null;
}

export function projectItems(): MenuItem[] {
  return [
    { action: "deleteProject", label: "Удалить проект" },
    { action: "checkProject", label: "Проверить индекс" },
  ];
}

/** Ближайшая строка файла либо карточка результата; папки собственного меню не имеют. */
export function documentAt(target: EventTarget | null): { path: string; line: number } | null {
  const element = target instanceof Element ? target.closest<HTMLElement>("[data-document-path]") : null;
  return element?.dataset.documentPath ? { path: element.dataset.documentPath, line: Number(element.dataset.documentLine ?? 0) } : null;
}

export function documentItems(): MenuItem[] {
  return [
    { action: "open", label: "Открыть" },
    { action: "editors", label: "Открыть в …" },
    { action: "reveal", label: /Mac/.test(navigator.platform) ? "Открыть в Finder" : "Открыть в Проводнике" },
    { action: "path", label: "Скопировать путь" },
    { action: "check", label: "Проверить индекс" },
  ];
}

const NON_TEXT_INPUTS = new Set(["checkbox", "radio", "button", "submit", "reset", "range", "color", "file", "image"]);

/** Поле, в которое можно вводить или из которого можно копировать текст. */
export function textField(target: EventTarget | null): HTMLInputElement | HTMLTextAreaElement | null {
  if (target instanceof HTMLTextAreaElement) return target;
  if (target instanceof HTMLInputElement && !NON_TEXT_INPUTS.has(target.type)) return target;
  return null;
}

/** Выделенный текст: в поле — по selectionStart/End, иначе — по выделению страницы. */
export function selectedText(target: EventTarget | null): string {
  const field = textField(target);
  if (field) return field.value.slice(field.selectionStart ?? 0, field.selectionEnd ?? 0);
  return window.getSelection()?.toString() ?? "";
}

/** Пункты меню для места клика; пустой список — меню не показывается. */
export function itemsFor(target: EventTarget | null): MenuItem[] {
  const field = textField(target);
  const copy: MenuItem = { action: "copy", label: "Копировать" };
  const paste: MenuItem = { action: "paste", label: "Вставить" };
  const all: MenuItem = { action: "selectAll", label: "Выделить всё" };
  if (field) {
    const items: MenuItem[] = [];
    if (selectedText(field)) items.push(copy);
    if (!field.readOnly && !field.disabled) items.push(paste);
    items.push(all);
    return items;
  }
  return selectedText(target) ? [copy] : [];
}

/** Вставляет текст в поле на место выделения и сообщает полю об изменении. */
export function insertInto(field: HTMLInputElement | HTMLTextAreaElement, text: string): void {
  const clean = field instanceof HTMLTextAreaElement ? text : text.replace(/\s*[\r\n]+\s*/g, " ");
  field.focus();
  field.setRangeText(clean, field.selectionStart ?? field.value.length, field.selectionEnd ?? field.value.length, "end");
  field.dispatchEvent(new Event("input", { bubbles: true }));
}

/** Выполняет пункт меню над полем (или над выделением страницы). Возвращает текст ошибки. */
export async function run(action: MenuAction, target: EventTarget | null): Promise<string | null> {
  const field = textField(target);
  if (action === "copy") {
    const text = selectedText(target);
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      document.execCommand("copy");
    }
    return null;
  }
  if (action === "selectAll") {
    if (field) {
      field.focus();
      field.select();
    } else {
      document.execCommand("selectAll");
    }
    return null;
  }
  if (!field) return null;
  try {
    insertInto(field, await navigator.clipboard.readText());
    return null;
  } catch {
    return "Не удалось прочитать буфер обмена — вставьте клавишами Ctrl+V";
  }
}

/** Положение меню: рядом с курсором, но внутри окна. */
export function place(x: number, y: number, width: number, height: number, viewW: number, viewH: number) {
  return {
    left: Math.max(4, Math.min(x, viewW - width - 4)),
    top: Math.max(4, Math.min(y, viewH - height - 4)),
  };
}
