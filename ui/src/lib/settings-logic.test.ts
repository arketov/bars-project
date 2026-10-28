import { describe, expect, it } from "vitest";
import {
  clone,
  embedFingerprint,
  estimate,
  isDirty,
  needsReindex,
  parseFingerprint,
  parseScore,
  validate,
} from "./settings-logic";
import type { SettingsDoc } from "./types";

const DOC: SettingsDoc = {
  indexing: { edit_pause_seconds: 30 },
  mcp: { graph_tools: false },
  embed: { accelerator: "auto", source: "builtin", model: "BAAI/bge-m3", base_url: null, api_key_name: null, api_key_env: null, batch_size: 32, unload_after_minutes: 15 },
  rerank: {
    accelerator: "auto",
    enabled: false,
    source: "builtin",
    model: "BAAI/bge-reranker-v2-m3",
    base_url: null,
    api_key_name: null,
    api_key_env: null,
    unload_after_minutes: 5,
  },
  search: {
    candidates: 30,
    rerank_candidates: 15,
    results: 5,
    results_reranked: 3,
    min_score_vector: 0,
    min_score_rerank: 0.1,
    agent_min_score: [0, 1],
    max_results: 20,
    max_candidates: 100,
    max_rerank_candidates: 50,
  },
  chunking: { min_chars: 250, max_chars: 6000 },
};

describe("settings-logic", () => {
  it("validates LM Studio identity and disallows enabled rerank", () => {
    const draft = clone(DOC);
    draft.embed.source = "lmstudio";
    draft.embed.model = "text-embedding-qwen3-embedding-0.6b";
    draft.embed.base_url = "http://127.0.0.1:1234/v1";
    expect(validate(draft)).toEqual({});
    expect(needsReindex(DOC, draft)).toBe(true);
    expect(parseFingerprint("lmstudio:model")).toEqual({source:"lmstudio",model:"model"});
    draft.rerank.source = "lmstudio";
    draft.rerank.enabled = true;
    expect(validate(draft)["rerank.source"]).toContain("LM Studio");
    draft.rerank.enabled = false;
    expect(validate(draft)["rerank.source"]).toBeUndefined();
  });
  it("supports Ollama embed identity but rejects enabled Ollama rerank", () => {
    const draft = clone(DOC);
    draft.embed.source = "ollama";
    draft.embed.model = "qwen3-embedding:0.6b";
    draft.embed.base_url = "http://127.0.0.1:11434";
    expect(validate(draft)).toEqual({});
    expect(needsReindex(DOC, draft)).toBe(true);
    expect(parseFingerprint("ollama:qwen3-embedding:0.6b")).toEqual({ source: "ollama", model: "qwen3-embedding:0.6b" });
    draft.rerank.enabled = true;
    draft.rerank.source = "ollama";
    expect(validate(draft)["rerank.source"]).toContain("Ollama");
    draft.rerank.enabled = false;
    expect(validate(draft)["rerank.source"]).toBeUndefined();
  });
  it("validates the indexing pause without requiring a rebuild", () => {
    const draft = clone(DOC);
    for (const seconds of [0, 30, 3600]) {
      draft.indexing.edit_pause_seconds = seconds;
      expect(validate(draft)["indexing.edit_pause_seconds"]).toBeUndefined();
      expect(needsReindex(DOC, draft)).toBe(false);
    }
    for (const seconds of [-1, 0.5, 3601, NaN, undefined]) {
      draft.indexing.edit_pause_seconds = seconds as number;
      expect(validate(draft)["indexing.edit_pause_seconds"]).toBeTruthy();
    }
  });
  it("independent device preferences do not change index identity", () => {
    const draft = clone(DOC);
    draft.embed.accelerator = "cpu";
    expect(needsReindex(DOC, draft)).toBe(false);
    draft.rerank.accelerator = "cpu";
    expect(needsReindex(DOC, draft)).toBe(false);
    expect(isDirty(DOC, draft)).toBe(true);
  });
  it("заводские значения проходят проверку", () => {
    expect(validate(DOC)).toEqual({});
  });

  it("срок выгрузки: 0 — можно, дробное, отрицательное и больше недели — нет", () => {
    const draft = clone(DOC);
    draft.embed.unload_after_minutes = 0;
    draft.rerank.unload_after_minutes = 10_080;
    expect(validate(draft)).toEqual({});
    expect(needsReindex(DOC, draft)).toBe(false);
    for (const bad of [-1, 1.5, 10_081, Number.NaN]) {
      draft.embed.unload_after_minutes = bad;
      draft.rerank.unload_after_minutes = bad;
      expect(Object.keys(validate(draft)).sort()).toEqual(["embed.unload_after_minutes", "rerank.unload_after_minutes"]);
    }
  });

  it("копия делается и с прокси, как у $state в Svelte", () => {
    const proxied = new Proxy(DOC, {});
    expect(() => structuredClone(proxied)).toThrow();
    expect(clone(proxied)).toEqual(DOC);
  });

  it("правка видна, возврат значения снимает её", () => {
    const draft = clone(DOC);
    expect(isDirty(DOC, draft)).toBe(false);
    draft.search.results = 7;
    expect(isDirty(DOC, draft)).toBe(true);
    draft.search.results = 5;
    expect(isDirty(DOC, draft)).toBe(false);
  });

  it("переиндексация нужна только при смене источника или модели", () => {
    const draft = clone(DOC);
    draft.search.results = 9;
    draft.embed.batch_size = 8;
    expect(needsReindex(DOC, draft)).toBe(false);
    draft.embed.model = "Qwen/Qwen3-Embedding-0.6B";
    expect(needsReindex(DOC, draft)).toBe(true);
    expect(embedFingerprint(draft.embed)).toBe("builtin:Qwen/Qwen3-Embedding-0.6B");
  });

  it("удалённый провайдер требует модель, адрес и ключ", () => {
    const draft = clone(DOC);
    draft.embed.source = "remote";
    draft.embed.model = "";
    const errors = validate(draft);
    expect(Object.keys(errors).sort()).toEqual(["embed.base_url", "embed.key", "embed.model"]);
    draft.embed.model = "text-embedding-3-small";
    draft.embed.base_url = "https://api.example.com/v1";
    draft.embed.api_key_env = "OPENAI_API_KEY";
    expect(validate(draft)).toEqual({});
    draft.embed.base_url = "ftp://x";
    expect(validate(draft)["embed.base_url"]).toBeDefined();
    draft.embed.base_url = "http://user:pw@host/";
    expect(validate(draft)["embed.base_url"]).toBeDefined();
  });

  it("локальный адрес ключа не требует, выключенный rerank не проверяется", () => {
    const draft = clone(DOC);
    draft.embed = { ...draft.embed, source: "local-openai", model: "bge-m3", base_url: "http://localhost:11434/v1" };
    draft.rerank = { ...draft.rerank, source: "remote", model: "" };
    expect(validate(draft)).toEqual({});
    draft.rerank.enabled = true;
    expect(validate(draft)["rerank.model"]).toBeDefined();
  });

  it("числа и оценки", () => {
    const draft = clone(DOC);
    draft.search.candidates = 0;
    draft.search.min_score_rerank = 1.5;
    draft.search.agent_min_score = [0.8, 0.2];
    draft.chunking.max_chars = 2.5;
    expect(Object.keys(validate(draft)).sort()).toEqual([
      "chunking.max_chars",
      "search.agent_min_score",
      "search.candidates",
      "search.min_score_rerank",
    ]);
    expect(parseScore("0,30")).toBe(0.3);
    expect(parseScore("abc")).toBeNaN();
  });

  it("отпечаток воркера разбирается, оценка времени — по OQ-58", () => {
    expect(parseFingerprint("remote:text-embedding-3-small")).toEqual({
      source: "remote",
      model: "text-embedding-3-small",
    });
    expect(parseFingerprint("что-то")).toBeNull();
    expect(estimate(100, "builtin:BAAI/bge-m3")).toEqual({ seconds: 50, note: "" });
    expect(estimate(100, "remote:x").note).toBe("зависит от провайдера");
    expect(estimate(100, "builtin:Qwen/Qwen3-Embedding-0.6B").seconds).toBeNull();
    expect(estimate(100, null).note).toBe("—");
  });
});
