---
title: Ступенчатая выдача поиска агенту — шаг 5.1.1
---

# Ступенчатая выдача поиска агенту — шаг 5.1.1

## Сделано

Шаг 5.1.1 плана: текстовый вид ответа `bars_rag_search` переписан по [проекту агентского слоя](../../features/agent-tools/agent-layer-project.md) — действующий контракт в [search-staging.md](../../features/agent-tools/search-staging.md). Имена инструментов и схемы параметров не менялись.

- **bars-core** (`crates/bars-core/src/search/hybrid.rs`): кандидаты с опорой ниже порога собирает `below_threshold`, при пустой выдаче `fold_weak` сворачивает до 3 разделов без загрузки текстов (`SearchResponse.weak`, `WeakHit`); порядок выдачи по `S` и `min(1, S)` в JSON прежние — окно не затронуто.
- **bars-protocol**: `WeakHit`, поля `SearchOutput.weak` и `query_words` (serde-default, совместимость версий); общий хелпер `split_other_headers`.
- **bars-worker**: заполнение `weak` и `query_words`; пустые поля в графовом режиме.
- **bars-mcp** (`crates/bars-mcp/src/render.rs`): шапка `N of M · scope: …`; строки `[1] 0.92 путь:a-b · Заголовок` с оценкой-опорой вместо `min(1, S)` (ISS-114); ступени — полный текст 1-го места и 2–3 при опоре rerank от 0,875, остальные отрывок до 300 знаков вокруг слов запроса, без rerank полные 1–2; блок `other headers` не больше восьми заголовков, совпавшие со словами запроса первыми, `(+N more)` (ISS-115); до трёх строк `weak:` при пустой выдаче; хвост только при отклонениях (индексация, очередь, прижатый предел, недоступный сигнал, пустой ответ, несколько папок).
- **Тесты**: слабые совпадения ядра (rerank/косинус/пустой пул фразой); семь тестов рендера (ступени rerank и cosine/none, отрывок с границами слов и регистром, предел other headers, weak, хвост); интеграционный тест MCP `default_format_is_text` переведён на новый формат; roundtrip `SearchOutput` в protocol.
- **Документы**: новый `search-staging.md`; правки `mcp-tools.md` (формат ответа и статус), `hybrid-search.md` (оценка-опора, поля ответа), `agent-layer-project.md` (статус), `Index.md`, `manual/agents.md`; **ISS-114 закрыт** (таблица исправленных в `docs/backlog/issues.md`), **ISS-115 частично** — предел `other headers` готов, поднятие определения с номером остаётся за шагом 5.4.1 с бенчем.
- Три ревью агента (пометка Sol) по производительности, логике и полноте: все находки закрыты, итоговое — «замечаний нет».
- Сборка и тесты изменённых крейтов выполнены в этой сессии с разрешения автора: `cargo fmt` чист по моим файлам (весь workspace — красный от долга 2.3.x), `cargo check --workspace` чист, clippy: мой код без замечаний (оставшиеся 3 ошибки — предсуществующие dead-test в `audits/contradictions.rs`), lib-тесты bars-protocol 16/16, bars-mcp 91/91, bars-core 464/466.

## Не сделано

- Параметр `detail` (`full`/`brief`) — придёт с инструментом `bars_search` (шаг 5.2.1), отмечено в search-staging.md.
- Сигнал номера в собственном заголовке раздела — шаг 5.4.1, принимается только прогоном бенча.
- Приёмка в установленной сборке (клиенты обновляются со следующей сборкой).

## Сложности

- **Предсуществующие красные тесты bars-core, не относящиеся к шагу** (подтверждено на чистом HEAD через stash): `storage::stem::tests::ukrainian_words_are_kept_as_written` и `search::hybrid::tests::identifier_goes_to_fts_as_a_phrase_words_and_a_whole_exact_form` (тест исправления OQ-285, писался без сборки). Автору: починить отдельно — иначе `check.ps1` встаёт на `cargo test`.
- **Предсуществующий формат-долг**: `cargo fmt --all --check` красный на HEAD в 14 файлах (шаги 2.3.x шли без fmt): examples `bench_quality.rs`, `quality/mod.rs`, `search/fulltext.rs`, `operators/tests.rs`, `settings/tests.rs`, `storage/stem.rs`, `protocol/launch.rs`, три файла тестов воркера. `check.ps1` встаёт на первом шаге; мои файлы отформатированы, чужой долг не трогал.
- **Предсуществующие ошибки clippy** (Rust 1.99, `-D warnings`): в `crates/bars-core/src/audits/contradictions.rs` три тестовые функции (строки 687, 746, 774) потеряли `#[test]` и «не используются» — тесты аудита противоречий молча не гонялись. Чужой модуль, не трогал; автору: включить `#[test]` и посмотреть, зелёные ли они.
- **Окружение сборки на машине**: cargo нет в PATH Git Bash (брал из `~/.cargo/bin`); `llama-cpp-sys-2` требует `VULKAN_SDK=C:/tools/VulkanSDK/1.4.363.0`, `CMAKE_GENERATOR=Ninja` (ninja — `C:\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\Ninja`), `CMAKE_BUILD_PARALLEL_LEVEL=1` и `LIBCLANG_PATH=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\Llvm\x64\bin` (libclang живёт в BuildTools, не в `C:\Program Files\LLVM` — штатная проверка `Test-Path` в check.ps1 его не находит). Первый прогон без них оставил в кэше `C:/bt/bars` CMake-конфиг с генератором VS — починил удалением промежуточного каталога `debug/build/llama-cpp-sys-2-*` и пересборкой (llama.cpp пересобрался ~2,5 мин).
- Ревью дважды ловило тестовые фикстуры (обрезка по байтам вместо знаков; `lines().nth()` у отрывка с переводами строк внутри окна) — код обе раз был прав.

## Дальше

- Шаг 5.2.1: четыре инструмента (`bars_search`, `bars_read`, `bars_status`, `bars_audit`) с английскими описаниями и `initialize`, удаление старых имён; затем 5.2.2 (skills и обе копии `BARS_INSTRUCTION.md`).
- Шаг 5.3.1: гейт по счётчику. Шаг 5.4.1: сигнал номера в заголовке с прогоном бенча.
- Автору: решить судьбу двух красных тестов и формат-долга 2.3.x — они блокируют зелёный `check.ps1` целиком.
