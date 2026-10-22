import { describe, expect, it } from "vitest";
import { documentHeadings, renderDocument, stripFrontmatter } from "./markdown";

it("структура учитывает frontmatter и не принимает код за заголовок", () => {
  expect(documentHeadings("---\ntitle: Тема\n---\n\n# Тема\n\n```md\n## Это код\n```\n\n## **Раздел**\n")).toEqual([{ line: 5, label: "Тема" }, { line: 11, label: "Раздел" }]);
});

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
  it("ссылка на другой .md — активная, внешняя и картинка — нет", () => {
    const html = renderDocument("[план](../features/x/plan.md#раздел), [сайт](https://x.y), [корень](/a.md)\n");
    expect(html).toContain('<a class="a" href="#" data-doc-link="../features/x/plan.md#раздел">план</a>');
    expect(html).toContain('<span class="a">сайт</span>');
    expect(html).toContain('<span class="a">корень</span>');
  });

  it("[[ссылка]] как в Obsidian — активная, с псевдонимом показывает псевдоним, в коде остаётся текстом", () => {
    const html = renderDocument("[[План]] и [[папка/Файл|подпись]] и `[[код]]`\n");
    expect(html).toContain('<a class="a" href="#" data-wiki-link="План">План</a>');
    expect(html).toContain('<a class="a" href="#" data-wiki-link="папка/Файл|подпись">подпись</a>');
    expect(html).toContain("<code>[[код]]</code>");
  });

  it("путь ссылки считается от документа и не выходит за корень проекта", async () => {
    const { resolveDocumentLink } = await import("./markdown");
    expect(resolveDocumentLink("docs/manual/new-topics.md", "../features/new-topics/plan.md#a")).toBe("docs/features/new-topics/plan.md");
    expect(resolveDocumentLink("docs/manual/new-topics.md", "Index.md")).toBe("docs/manual/Index.md");
    expect(resolveDocumentLink("docs/manual/a.md", "../../QUESTIONS.md")).toBe("QUESTIONS.md");
    expect(resolveDocumentLink("docs/a.md", "../../x.md")).toBeNull();
    expect(resolveDocumentLink("docs/a.md", "https://x.y/b.md")).toBeNull();
    expect(resolveDocumentLink("docs/a.md", "b.png")).toBeNull();
  });

  it("чанк рисуется, краткий текст — без разметки", async () => {
    const { renderChunk, plainText } = await import("./markdown");
    expect(renderChunk("- **а**\n- `б`")).toContain("<li>");
    expect(plainText("## Заголовок\n\n- **жирный** и `код`\n\n[ссылка](http://x)")).toBe(
      "Заголовок жирный и код ссылка",
    );
  });
});
