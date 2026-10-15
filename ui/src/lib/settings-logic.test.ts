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
  embed: { source: "builtin", model: "BAAI/bge-m3", base_url: null, api_key_name: null, api_key_env: null, batch_size: 32 },
  rerank: {
    enabled: false,
    source: "builtin",
    model: "BAAI/bge-reranker-v2-m3",
    base_url: null,
    api_key_name: null,
    api_key_env: null,
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
  device: { accelerator: "auto" },
  chunking: { min_chars: 250, max_chars: 6000 },
};

describe("settings-logic", () => {
  it("заводские значения проходят проверку", () => {
    expect(validate(DOC)).toEqual({});
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
