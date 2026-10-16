import { describe, expect, it } from "vitest";
import { insertInto, itemsFor, place } from "./contextmenu";

function field(tag: "input" | "textarea", value: string, start = 0, end = 0) {
  const el = document.createElement(tag);
  document.body.append(el);
  el.value = value;
  el.setSelectionRange(start, end);
  return el;
}

describe("itemsFor", () => {
  it("в поле без выделения: вставить и выделить всё", () => {
    expect(itemsFor(field("input", "текст")).map((i) => i.action)).toEqual(["paste", "selectAll"]);
  });
  it("в поле с выделением добавляется копирование", () => {
    expect(itemsFor(field("textarea", "текст", 0, 2)).map((i) => i.action)).toEqual(["copy", "paste", "selectAll"]);
  });
  it("в поле только для чтения нет вставки", () => {
    const el = field("input", "abc", 0, 1);
    el.readOnly = true;
    expect(itemsFor(el).map((i) => i.action)).toEqual(["copy", "selectAll"]);
  });
  it("флажок и обычный элемент без выделения — меню нет", () => {
    const box = document.createElement("input");
    box.type = "checkbox";
    document.body.append(box);
    expect(itemsFor(box)).toEqual([]);
    expect(itemsFor(document.createElement("div"))).toEqual([]);
  });
});

describe("insertInto", () => {
  it("вставляет на место выделения и шлёт input", () => {
    const el = field("textarea", "abcd", 1, 3);
    let fired = 0;
    el.addEventListener("input", () => fired++);
    insertInto(el, "X");
    expect(el.value).toBe("aXd");
    expect(fired).toBe(1);
  });
  it("в однострочное поле вставляет без переносов", () => {
    const el = field("input", "", 0, 0);
    insertInto(el, "а\nб");
    expect(el.value).toBe("а б");
  });
});

describe("place", () => {
  it("не выпускает меню за границы окна", () => {
    expect(place(990, 790, 200, 100, 1000, 800)).toEqual({ left: 796, top: 696 });
    expect(place(10, 10, 200, 100, 1000, 800)).toEqual({ left: 10, top: 10 });
  });
});
