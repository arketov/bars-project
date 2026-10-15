import { describe, expect, it } from "vitest";
import { renderDocument, stripFrontmatter } from "./markdown";

const DOC = `---
title: Поиск
---

# Поиск

Первый абзац.

## Порог

Порог применяется к оценке.

- пункт один
- пункт два

## Хвост

Последний раздел.
`;

describe("renderDocument", () => {
  it("frontmatter не рисуется и не сдвигает номера строк", () => {
    const { body, lines } = stripFrontmatter(DOC);
    expect(body.startsWith("\n# Поиск")).toBe(true);
    expect(lines).toBe(3);
    // «## Порог» на 9-й строке файла: подсветка по строкам 9–14 включает раздел.
    const html = renderDocument(DOC, { first: 9, last: 14 });
    expect(html).toContain('<section class="hit"');
    expect(html).toContain("строки 9–14 · найдено");
    expect(html.indexOf("hit")).toBeLessThan(html.indexOf("Порог"));
    expect(html.indexOf("Последний раздел")).toBeGreaterThan(html.indexOf("</section>"));
    expect(html).not.toContain("title: Поиск");
  });

  it("без подсветки секции нет", () => {
    expect(renderDocument(DOC)).not.toContain("hit");
  });

  it("подсветка вне документа ничего не отмечает", () => {
    expect(renderDocument(DOC, { first: 500, last: 600 })).not.toContain('class="hit"');
  });

  it("сырой HTML показывается текстом, ссылки не активны", () => {
    const html = renderDocument("<script>alert(1)</script>\n\nтекст [ссылка](http://x.y) и <b>жирный</b>\n");
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).not.toContain("<a ");
    expect(html).not.toContain("<b>");
  });
});

describe("renderChunk и plainText", () => {
  it("чанк рисуется, краткий текст — без разметки", async () => {
    const { renderChunk, plainText } = await import("./markdown");
    expect(renderChunk("- **а**\n- `б`")).toContain("<li>");
    expect(plainText("## Заголовок\n\n- **жирный** и `код`\n\n[ссылка](http://x)")).toBe(
      "Заголовок жирный и код ссылка",
    );
  });
});
