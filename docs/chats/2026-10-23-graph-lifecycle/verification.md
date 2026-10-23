---
title: Проверки обновления GraphRAG 4.1.3
---

# Проверки обновления GraphRAG 4.1.3

Проверено 23-10-2026 на Windows, в текущем общем дереве с параллельными изменениями. Продуктовый контракт — [правила обновления](../../features/graphrag/implementation.md).

## Проверки кода и интерфейса

| Проверка | Результат |
|---|---|
| `cargo test -p bars-graph --all-features --target-dir C:/bt/bars-graph-lifecycle` | 59 прошли; `graph-tests.txt` |
| `cargo test -p bars-worker graph --lib --target-dir C:/bt/bars` | 8 прошли; `worker-tests.txt`, финальная проверка после review — `worker-final.txt` |
| `cargo test -p bars-worker vanished_project_folder_is_unavailable --lib --target-dir C:/bt/bars` | 1 прошёл после исправления подготовки: ждёт окончание графа, а не только готовность текста |
| `cargo test -p bars-cli --target-dir C:/bt/bars` | 4 unit + 9 integration прошли, включая GraphRAG по настоящему IPC |
| `npm --prefix ui test -- --run src/lib/graph/PhysicalGraph.test.ts src/lib/GraphPage.test.ts` | 16 прошли; `ui-tests.txt` |
| `npm --prefix ui run check`, `npm --prefix ui run build` | 0 ошибок/предупреждений Svelte; production собран; прежние предупреждения размера чанков/dynamic import — `ui-build.txt` |
| `cargo clippy -p bars-worker -p bars-cli --lib --message-format short --target-dir C:/bt/bars` | Завершился; прежние too_many_arguments, collapsible_if, large_enum_variant и предупреждения других модулей — `clippy.txt`; строгий `-D warnings` не заявляется |

Общий worker-прогон: 278 прошли, 2 упали, 1 ignored. Упал тест удаления свободной папки: после выделения графа в фон состояние Ready текста перестало означать закрытый SQLite. Исправлен порядок регистрации задания до Ready и подготовка теста; целевой повтор прошёл. Второй сбой — `settings_io::tests::tei_settings_roundtrip_both_models`: валидация имени модели TEI; граф этот путь не меняет, параллельная работа над провайдерами не присвоена. Полный workspace зелёным не объявляется.

## Нативная проверка

`graph_lifecycle_probe` использует настоящий Tauri/WebView2 и `commands::bars_call`, временный проект/профиль и FakeProvider. Запуск со скрытым окном, программные запросы; не физическая или визуальная приёмка.

Сборка: `cargo rustc --release -p bars-app --example graph_lifecycle_probe -- -C link-arg=/MANIFEST:EMBED -C link-arg=/MANIFESTINPUT:<абсолютный путь crates/bars-app/examples/workspace_probe.manifest>`. Исполняемый файл запускается с аргументом собственного временного каталога; `BARS_USER_HOME` процесса указывает туда же. Результат копируется из `result.json`.

`native.json`: 9 проверок — структура готова, смысловая полнота partial, прогресс publish, отсутствие файловых ошибок, новое поколение после правки, snapshot_expired для старого снимка, accepted явного build, стабильность снимка неизменённых источников и сохранение других поколений при `layer=structure`.

Первый стенд пропустил обязательный cwd; `native-first.json` сохраняет неуспех, после исправления передаются cwd и project. Повтор не менял ожиданий продукта. Живой bars, рабочие профили и индексы не использовались.

## Review и границы

Ручной review выявил и исправил: атомарное чтение статуса; проверку ревизий исключённых файлов; ошибочное нулевое coverage.failed; гонку снятия active job после освобождения job gate; накопление завершённых task handles; отказ структурных путей при неготовом semantic layer. Полезность извлечения и бюджеты 256 МиБ/100 мс этим набором не доказаны.

Проверка whitespace собственных файлов чистая; общий diff PLAN.md содержит прежний пробел на строке 70 вне изменения 4.1.3. Уборка двух собственных временных каталогов нативного стенда отклонена автоматической проверкой (`blocked by policy`); результаты сохранены, каталоги оставлены, пути записаны в handoff.

Установленная версия остаётся непроверенной: автор 23-10-2026 сохранил запрет установки OQ-113. Части 4.1.3 отмечены выполненными, общий пункт открыт по этой границе. Стадия качества GraphRAG и IDEA-11 не закрыты.
