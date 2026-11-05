---
title: Handoff — сложение баллов, операторы и свёртка разделов, шаг 2.3.4
---

# Handoff — сложение баллов, операторы и свёртка разделов, шаг 2.3.4

Продолжает [handoff полнотекстовых сигналов](../2026-11-05-fulltext-signals/handoff.md) и [handoff наборов с операторами](../2026-11-05-bench-no-answer-operators/handoff.md).

## Сделано

- **Код `bars-core`, не собран и не запущен.**
  - `search/hybrid.rs` переписан: пул кандидатов (полнотекстовый, идентификаторы, векторный, расширение `mix`), `f` и `p`, предварительная оценка `S0`, rerank первых `rerank_candidates`, итог `S = a + w_c·c + w_f·f + w_p·p`, порог по опоре, свёртка кусков раздела. Слияния RRF и правила «идентификаторы первыми» больше нет.
  - `search/operators.rs` (новый): `ParsedQuery::parse` (фраза, `-слово`, `-"фраза"`, `path:`/`папка:`), `OperatorFilter` (условия в выражении FTS5 и проверка векторных кандидатов по `rowid`). `Scope::build_with_paths` — `path:` как пересечение с `modules`/`files`, неизвестный путь — `invalid_params`.
  - `search/fulltext.rs`: сигнал `p` (`p_signal`, `QueryTerms::exact_probe`), `fts_plan_for`. `search/params.rs`: `ScoreWeights`, `SearchSettings.min_score` и `weights` вместо двух порогов, `ScoreKind::None`, `AppliedParams.weights/signals`. `settings/raw.rs`: перенос `min_score_vector`/`min_score_rerank` в `min_score`.
- **Протокол и воркер:** `HitSignals`, `ScoreWeights`, `UnderstoodQuery`, поля результата `signals`, `parts`, `parts_found`, `applied.weights/signals`, `understood`, `ScoreKind::None`, `SearchDoc.min_score`; `settings_io` заменяет старые ключи одним при сохранении; `similar` отдаётся без баллов. MCP (оба рендерера), CLI — строки `signals:`, `parts:`, `understood:`, `weights:` и русские «баллы», «Понято как»; описания параметров `query` и `min_score` у агента.
- **Бенч:** `bench_quality` выполняет для запросов с `operators` парный запрос без операторов (`mode: paired`), проверка трассы сравнивает лучшие куски с пулом; `quality/mod.rs`, примеры бенчей переведены на `min_score`.
- **Тесты, не запускались** (по критериям приёмки): `search/operators/tests.rs` (разбор), `search/hybrid/tests.rs` (фраза, исключение слова и фразы, векторные кандидаты, `path:` и пересечение, `invalid_params`, обычные символы, свёртка, число разделов, порог по опоре, режим без опоры), `settings/tests.rs` (перенос порогов, умолчание 0,1), `search_signals.rs` и тест протокола, `settings_io`.
- **Документы:** [гибридный поиск](../../features/search/hybrid-search.md) переписан под реализацию (схема, операторы, свёртка, форма ответа, стартовые веса); правки `fulltext.md`, `settings.md`, `chunking.md`, `architecture/protocol.md`, `mcp-tools.md`, `manual/search.md` (новый раздел об операторах), `manual/how-it-works.md`, `knowledge/search-quality.md`, `global-search-design.md` (ссылка на форму ответа), оглавления, skill `bars-docs` (строка `bars_rag_search`); вопрос OQ-278.

## Форма ответа для шага 2.4.2

Подробно — раздел «Форма ответа поиска» в [гибридном поиске](../../features/search/hybrid-search.md). Коротко: `results` — плоский список разделов по убыванию `score`, карточку документа окно собирает группировкой по `path`; у результата `signals` (`rerank?`, `cosine?`, `fulltext`, `exact`), `parts`, `parts_found`, `first_line`/`last_line` раздела, `text` лучшего куска; `score_kind` — `rerank`, `cosine` или `none` (опоры нет, оценка не рисуется); `applied.weights`, `applied.signals`, `applied.min_score`; `understood` (`text`, `phrases`, `excluded_words`, `excluded_phrases`, `paths`) есть только при операторах. Смещений совпадений для подсветки ответ не несёт.

## Не сделано

- Ничего не собиралось, не тестировалось: `cargo`, `check.ps1`, бенч не запускались. Код не коммитился (коммит кода — после зелёного `check.ps1`); в рабочем дереве он лежит рядом с чужими правками (`hybrid.rs`, `storage/index.rs`, `stem.rs`, тесты поиска правили шаги 2.3.3 и 2.3.6).
- **Окно (`ui/`) не менялось, и страница настроек в нём сломана до шага 2.4.2:** `SearchDoc` отдаёт `min_score` вместо `min_score_vector`/`min_score_rerank` (`ui/src/lib/types.ts`, `SettingsPage.svelte`, `settings-logic.ts` и их тесты, `mock.ts`), а у результата и `applied.score_kind` появилось значение `none`. Новые поля ответа окно пока игнорирует.
- `w_l`/сигнал связей не считается (шаг 2.3.5), подбор весов и порога — шаг 2.3.7. Веса не выставляются в `settings.toml` (OQ-278).
- Добор разделов после свёртки не делается: если разделов меньше N, выдача короче; пул с rerank — `rerank_candidates` кусков.

## Сложности

- Рабочее дерево содержало чужие незакоммиченные правки тех же файлов: они сохранены, правки наложены сверху. Файлы с CRLF править пришлось скриптами с сохранением перевода строки.
- Мелкие решения агента: вектор берёт втрое больше кандидатов, если в запросе есть фраза или исключение (отсев операторами не оставляет пул пустым); при rerank итог раздела сравнивается по `S` лучшего куска (бонус `fold_bonus` по умолчанию 0); у `similar` баллов по сигналам и счёта кусков нет; фраза ищется по колонке точной формы, где целые формы идентификаторов стоят среди слов.
- Ожидаемые места первой правки при сборке: `hybrid/tests.rs` (кусочная нарезка в тестах свёртки зависит от `chunking`, ожидание строк раздела), тесты воркера с умолчанием порога 0,1 вместо 0 для косинуса и FakeProvider.

## Дальше

Автор собирает и запускает `check.ps1` → при красном правка по списку выше → шаг 2.4.2 (окно: форма ответа выше, страница настроек, «Понято как», переключатель баллов) и 2.3.5 (связи, `w_l`) → в 2.3.7 один прогон со сборкой: подбор весов и порога, строки `paired` для точности операторов; ответ автора на OQ-278.
