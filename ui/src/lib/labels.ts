// Подписи окна по OQ-217/ISS-59. Wire-значения сохраняются в подсказках, данные не меняются.
import type { GraphOperation, GraphView, ObjectKind, RetrievalMode } from "./graph";
import type { EdgeKind, LayerState, Mode } from "./graph/model";
import type { ScoreKind } from "./types";

export interface Label { text: string; hint: string }
const entry = (text: string, hint = ""): Label => ({ text, hint });
export const labels = {
  searchMode: {
    hybrid: entry("Обычный"), mix: entry("Со связями"), graph: entry("Отношения и пути"),
  } satisfies Record<RetrievalMode, Label>,
  score: {
    rerank: entry("Оценка модели", "Модель перечитывает найденные разделы вместе с запросом"),
    cosine: entry("Оценка по близости", "Смысловая близость к запросу; порядок учитывает также совпадения слов"),
    none: entry("Оценки нет"),
  } satisfies Record<ScoreKind | "none", Label>,
  layerState: {
    ready: entry("готово"), building: entry("строится"), partial: entry("частично"), stale: entry("устарело"),
    failed: entry("ошибка"), cancelled: entry("отменено"), not_built: entry("не построено"), disabled: entry("выключено"),
  } satisfies Record<LayerState, Label>,
  completeness: {
    complete: entry("Расчёт завершён полностью"), partial: entry("Есть известные пропуски"),
    unknown: entry("Полнота отношений не установлена"),
  },
  graphJob: {
    idle: entry("Ожидает изменений"), queued: entry("Ожидает выполнения"), running: entry("Обновляется"),
    stopped: entry("Построение остановлено · После новых изменений граф обновится автоматически"),
    failed: entry("Не удалось обновить граф"),
  },
  edgeKind: {
    markdown_link: entry("Ссылки"), document_similarity: entry("Близость документов"),
    chunk_similarity: entry("Близость разделов"), query_result: entry("Связь с запросом"),
  } satisfies Record<EdgeKind, Label>,
  graphMode: {
    links: entry("Ссылки"), meaning: entry("По смыслу"), chunks: entry("По смыслу · разделы"), all: entry("Все связи"),
  } satisfies Record<Mode, Label>,
  graphView: {
    documents: entry("Документы"), sections: entry("Заголовки"), chunks: entry("Разделы"),
    entities: entry("Сущности"), mixed: entry("Все объекты"),
  } satisfies Record<GraphView, Label>,
  nodeKind: {
    document: entry("Документ"), section: entry("Заголовок"), chunk: entry("Раздел"), entity: entry("Сущность"),
    community: entry("Тема"), query: entry("Запрос"),
  } satisfies Record<ObjectKind | "query", Label>,
  operation: {
    graph: entry("Карта"), relations: entry("Связи"), paths: entry("Путь"), impact: entry("Влияние"),
    context: entry("Контекст"), explore: entry("Обзор"),
  } satisfies Record<GraphOperation, Label>,
  role: {
    requirement: entry("Требования"), decision: entry("Решения"), behavior: entry("Поведение"),
    architecture: entry("Архитектура"), constraint: entry("Ограничения"), verification: entry("Проверки"),
  },
  predicate: {
    links_to: entry("ссылается на"), depends_on: entry("зависит от"), uses: entry("использует"),
    implements: entry("реализует"), verifies: entry("проверяет"), supersedes: entry("заменяет"),
    describes: entry("описывает"), contradicts: entry("противоречит"), requires: entry("требует"),
    related_to: entry("связано с"), similar_to: entry("близко по смыслу к"),
    contains: entry("содержит"), part_of: entry("входит в"),
    mentions: entry("упоминает"), covers: entry("покрывает"), next_chunk: entry("следует за"),
    co_occurs: entry("встречается рядом с"), member_of: entry("входит в"),
  },
  limit: {
    max_nodes: entry("достигнут предел числа узлов"), nodes: entry("достигнут предел числа узлов"),
    max_edges: entry("достигнут предел числа связей"), edges: entry("достигнут предел числа связей"),
    max_bytes: entry("достигнут предел размера ответа"), bytes: entry("достигнут предел размера ответа"),
    limit: entry("достигнут предел числа результатов"), items: entry("достигнут предел числа результатов"),
    fields: entry("длинные поля сокращены"), skipped: entry("часть документов пропущена"),
    max_depth: entry("достигнут предел глубины"), depth: entry("достигнут предел глубины"),
    max_paths: entry("достигнут предел числа путей"), paths: entry("достигнут предел числа путей"),
    max_fanout: entry("достигнут предел числа соседей"), fanout: entry("достигнут предел числа соседей"),
    timeout: entry("достигнут предел времени"), timeout_ms: entry("достигнут предел времени"),
    token_budget: entry("достигнут предел объёма контекста"),
  },
  fallback: {
    "Связи отключены: include_related=false": entry("Связанные источники выключены"),
    "include_related=false": entry("Связанные источники выключены"),
  },
  motion: {
    running: entry("Движение идёт"), idle: entry("Раскладка в покое"), stopped: entry("Остановлено вами"),
    reduced: entry("Без движения (системная настройка)"),
  },
  action: {
    checkProject: entry("Проверить проект", "Сверит документы с поиском и обновит изменившиеся"),
    checkDocument: entry("Проверить документ", "Сверит документ с поиском и обновит изменения"),
  },
};
export type LabelGroup = keyof typeof labels;
const unknown: Record<LabelGroup, string> = {
  searchMode: "Режим не определён", score: "Вид оценки не определён", layerState: "состояние не определено",
  edgeKind: "Вид связи не определён", graphMode: "Режим не определён", graphView: "Вид не определён",
  nodeKind: "Объект", operation: "Операция", role: "Роль не определена", predicate: "связано с",
  limit: "причина ограничения не определена", fallback: "Причина изменения режима не определена",
  motion: "Состояние движения не определено", action: "Действие не определено",
  completeness: "Полнота отношений не установлена", graphJob: "Состояние обновления не определено",
};

/** Открытые wire-строки могут быть новее окна; неизвестное не попадает в видимую подпись. */
export function label(group: LabelGroup, id: string): Label {
  const table: Readonly<Record<string, Label>> = labels[group];
  const found = Object.hasOwn(table, id) ? table[id] : undefined;
  return { text: found?.text ?? unknown[group], hint: [found?.hint, id].filter(Boolean).join(" · ") };
}

// Целые известные фразы, без вырезания частей идентификатора регулярными выражениями.
const phrases: Readonly<Record<string, string>> = {
  "число документов ограничено": "достигнут предел числа документов",
  "число чанков ограничено": "достигнут предел числа разделов",
  "число связей ограничено": "достигнут предел числа связей",
};
/** Свободное русское пояснение сохраняется; диагностические строки доступны через title. */
export function explanation(group: "limit" | "fallback", value: string): Label {
  if (Object.hasOwn(labels[group], value)) return label(group, value);
  if (Object.hasOwn(phrases, value)) return { text: phrases[value], hint: value };
  // Проверка алфавита, а не подстановка фрагментов: неизвестную диагностику не маскируем обрезкой.
  const readable = !/[A-Za-z_=]/u.test(value);
  return readable && value.trim() ? { text: value, hint: value } : label(group, value);
}

export const limitText = (reasons: readonly string[]): string =>
  [...new Set(reasons.map(reason => explanation("limit", reason).text))].join("; ") || "причина ограничения не определена";

/** Только служебные пояснения графа; названия, цитаты и текст документов сюда не передаются. */
export function graphNote(value: string): Label {
  const readable = !/[A-Za-z_=]/u.test(value);
  const terms: Readonly<Record<string, string>> = {
    "Чанков": "Разделов", "чанков": "разделов", "чанки": "разделы", "чанка": "раздела", "чанк": "раздел",
  };
  let text = readable ? value : "Подробности состояния графа доступны в подсказке";
  if (readable) for (const [term, word] of Object.entries(terms)) text = text.replaceAll(term, word);
  return { text, hint: value };
}
