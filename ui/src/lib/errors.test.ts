import { describe, expect, it } from "vitest";
import { explainBuiltin, explainProvider, screenFor, sourceOf } from "./errors";
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

  it("встроенная модель: скачивание или файлы", () => {
    expect(explainBuiltin("не удалось скачать с huggingface.co").kind).toBe("model-download");
    expect(explainBuiltin("файл model.onnx не найден").kind).toBe("model-files");
  });

  it("экран ошибки поиска зависит от источника модели", () => {
    const error = new BarsError("api: 401", "provider_error", -32008);
    expect(screenFor(error, "remote").kind).toBe("provider");
    expect(screenFor(error, "builtin").kind).toBe("model-files");
    expect(screenFor(new BarsError("странно", "invalid_params"), "remote").kind).toBe("other");
  });
});
