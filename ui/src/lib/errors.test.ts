import { describe, expect, it } from "vitest";
import { builtinModelId, builtinReason, explainProvider, screenFor, sourceOf } from "./errors";
import { BarsError } from "./types";

describe("errors", () => {
  it("источник модели по отпечатку", () => {
    expect(sourceOf("builtin:BAAI/bge-m3")).toBe("builtin");
    expect(sourceOf("remote:text-embedding-3-small")).toBe("remote");
    expect(sourceOf("local-openai:nomic")).toBe("local-openai");
    expect(sourceOf("")).toBeNull();
    expect(sourceOf(undefined)).toBeNull();
  });

  it("причины провайдера из таблицы error-states", () => {
    expect(explainProvider("api.example.com: 401 Unauthorized")).toContain("Ключ не принят");
    expect(explainProvider("запрос к провайдеру: таймаут")).toContain("повторите позже");
    expect(explainProvider("сервер ответил 503")).toContain("повторите позже");
    expect(explainProvider("вход слишком длинный")).toContain("размер чанка");
    expect(explainProvider("что-то странное")).toContain("адрес и ключ");
  });

  it("вид ошибки встроенной модели берётся из data.reason, неизвестное — runtime", () => {
    expect(builtinReason("network")).toBe("network");
    expect(builtinReason("files_missing")).toBe("files_missing");
    expect(builtinReason("out_of_memory")).toBe("out_of_memory");
    expect(builtinReason("runtime")).toBe("runtime");
    expect(builtinReason(undefined)).toBe("runtime");
    expect(builtinReason("что-то новое")).toBe("runtime");
  });

  it("экран встроенной модели по каждому виду: свой заголовок, текст не влияет", () => {
    const titles = {
      network: "Не удалось скачать модель поиска",
      files_missing: "Файлы модели не найдены или повреждены",
      out_of_memory: "Модели не хватило памяти",
      runtime: "Встроенная модель не отработала",
    } as const;
    for (const [reason, title] of Object.entries(titles)) {
      // Слова про файлы и сеть в тексте ошибки вид не меняют.
      const error = new BarsError("файл model.onnx не найден, нет сети huggingface.co", "provider_error", -32008, { reason });
      const screen = screenFor(error, "builtin");
      expect(screen.kind).toBe("builtin");
      expect(screen.title).toBe(title);
    }
    const screen = screenFor(new BarsError("сбой ORT", "provider_error", -32008, { reason: "out_of_memory" }), "builtin");
    expect(screen.kind === "builtin" && screen.reason).toBe("out_of_memory");
    expect(screen.hint).toContain("другое устройство");
  });

  it("идентификатор встроенной модели проекта по отпечатку", () => {
    expect(builtinModelId("builtin:BAAI/bge-m3")).toBe("BAAI/bge-m3");
    expect(builtinModelId("remote:text-embedding-3-small")).toBeNull();
    expect(builtinModelId("builtin:")).toBeNull();
    expect(builtinModelId(undefined)).toBeNull();
  });

  it("model_loading показывает ход скачивания и не считается ошибкой", () => {
    const loading = screenFor(new BarsError("грузится", "model_loading", -32009, { percent: 42 }), "builtin");
    expect(loading.kind).toBe("loading");
    expect(loading.title).toBe("Модель грузится, 42 %");
    expect(screenFor(new BarsError("грузится", "model_loading", -32009, { percent: 100 }), "builtin").title).toBe(
      "Модель загружается в память",
    );
  });
});
