---
title: Протокол канала bars — методы, параметры, ошибки
---

# Протокол канала bars — методы, параметры, ошибки

Контракт крейта `bars-protocol`: что `bars-mcp` и `bars-cli` отправляют воркеру по локальному каналу ([ipc.md](ipc.md)) и что получают в ответ. Решение агента, 14-10-2026.

## Кто пользуется протоколом bars

- **`bars-mcp`** превращает каждый MCP-инструмент в один вызов метода ([features/agent-tools/mcp-tools.md](../features/agent-tools/mcp-tools.md)).
- **`bars-cli`** превращает каждую команду в один вызов метода ([features/agent-tools/cli.md](../features/agent-tools/cli.md)). Флаг `--json` печатает ответ метода как есть.
- **Окно** живёт в том же процессе, что и воркер, и канал не использует. Его команды Tauri вызывают те же обработчики, что и методы протокола, поэтому окно, MCP и CLI получают одинаковые ответы (Т-20).

## Формат сообщений канала

Внутренний метод окна `project_observe {window, project}` подтверждает открытый проект; `project: null` снимает привязку окна. Ответ — `{saved: true}`. Он недоступен клиентам MCP и CLI по каналу. Сроки и правила активности описаны в [обновлении индекса](../features/indexing/updates.md).

JSON-RPC 2.0, одно сообщение на строку, UTF-8. На одном соединении можно делать несколько вызовов по очереди. `bars-mcp` переиспользует пул до четырёх соединений в сессии; прерванный обмен закрывает свой канал. `bars-cli` открывает канал на одну команду.

Первый вызов на соединении — `hello`. Без него воркер отвечает на любой метод ошибкой `protocol_mismatch`.

## Как метод находит проект

### Текстовый предел GraphRAG в IPC

`graph_query` и `graph_search` принимают внутренний необязательный `text: bool` (по умолчанию `false`). MCP/CLI включают его только для текстового вывода: воркер ограничивает элементы общим рендером `bars-protocol::graph_text` до публикации курсора. DTO GraphRAG v1 и JSON-вызовы сохраняются; окно получает исходный DTO без текстового ограничения.

### Исходный MCP-контекст в IPC

Для учёта отдельных вызовов `bars-mcp` добавляет разрешённый исходный вход в необязательное поле `params._mcp`, до подстановки типизированных умолчаний. В нём сохраняется различие пропущенного поля, null и явного значения; `format` передаётся как поле прокси. Worker повторно применяет allowlist. Это контекст наблюдения, не новое разрешение на методы и не изменение параметров поиска.

`Handler::handle_observed` получает этот контекст вместе с ID соединения после допуска сессией. Отсутствие контекста обозначает неполноту истории. Если добавление `_mcp` превышает лимит строки IPC, клиент опускает контекст, сохраняя допустимость самого вызова. Ответ инструмента и версия протокола от наличия контекста не меняются. Состав сохраняемых данных и границы ошибок — [история MCP](../features/window/mcp-statistics.md).

### Разрешение цели вызова

У методов, которые работают с проектом, три параметра (`Target`: `cwd`, `project?`, `wt?`):

- `cwd` — рабочая папка клиента. `bars-mcp` передаёт свою cwd, то есть папку сессии агента; `bars-cli` — папку, из которой его запустили;
- `project` — необязательный явный проект; строки, которые его выбирают, — таблица ниже;
- `wt` — необязательный вид проекта: git worktree (Т-48; проект, реализации нет).

Если `project` не задан, проект — ближайшая к `cwd` зарегистрированная папка вверх по дереву (Т-15); сессия с `cwd` в живом git worktree получает worktree проекта-владельца (OQ-307). Не нашлось — ошибка `project_not_found`. Как проекты регистрируются и как называются — [projects.md](../features/projects/projects.md).

Строки, которыми `project` выбирает проект (Т-49; написано шагом 6.2.1 без проверки сборкой, кроме регистрации неизвестного подпроекта; контракт — [подпроекты](../features/projects/subprojects.md)):

| Строка | Кого выбирает |
|---|---|
| абсолютный путь | запись с этим корнем, всегда точнее имени |
| составное имя `главный/путь` | подпроект (проект с зарегистрированным предком); `\` приравнивается к `/` |
| собственное отображаемое имя | проект без предка или самостоятельный подпроект; у найденного автоматически подпроекта своего имени нет |

Несколько записей под одним именем — ошибка с путями. Неизвестное имя с первым сегментом — главным проектом даёт `project_not_found` с перечнем его подпроектов (до 10; написано); без такого префикса — прежний текст со ссылкой на `bars_status(all=true)`. Неизвестный селектор, указывающий на папку внутри зарегистрированного проекта вне его корня документации и отвечающую правилу подпроекта, воркер регистрирует ленивым подпроектом и повторяет разбор, а для папки, не отвечающей правилу, называет причину (проект, реализации нет: шаг автопоиска). Модуль и подпапка в `project` не задаются: для них `in`.

Параметр `wt` относится к проекту вызова и принимает имя worktree (папка записи `.git/worktrees/<имя>`), абсолютный путь рабочей копии или `"."` — основной checkout. Без `wt` значение берётся из `cwd`: внутри живого worktree владельца — этот worktree, иначе основной checkout; явный `project`, не равный владельцу копии, неявного `wt` не даёт. Имя ветки как значение не принимается. Неизвестный `wt` — `worktree_not_found` со списком worktree проекта. Правила, дельта индекса и разрешение в реестре воркера — [git worktree как вид проекта](../features/projects/worktrees.md).

## Методы протокола bars

| Метод | Параметры | Ответ | Кто вызывает |
|---|---|---|---|
| `hello` | `protocol`, `client` (`mcp` / `cli`), `client_version` | `protocol`, `worker_version`, `worker_build` | все |
| `search` | `cwd`, `project`, `query`, `modules`, `files`, `limit`, `candidates`, `min_score`, `wait_models` (необязательное; `false` — не ждать загрузку моделей, шлёт только окно: [ожидание моделей](../features/search/hybrid-search.md#ожидание-моделей)) | результаты и применённые параметры ([features/search/hybrid-search.md](../features/search/hybrid-search.md)) | MCP, CLI |
| `similar` | `cwd`, `project`, `chunk_id`, `modules`, `limit` | исходный чанк, ближайшие чанки с косинусной близостью и применённый `limit` ([features/search/maintenance-tools.md](../features/search/maintenance-tools.md)); неизвестный или устаревший ID — `chunk_not_found` | MCP, CLI |
| `duplicates` | `cwd`, `project`, `modules`, `limit` | записи о повторах (`kind`, чанки с ID, `score`, `shared`, `differences`, `repeated_block`), `counts`, `total`, применённый `limit` и `clamped`, `chunks_checked`, `chunks_skipped_short` ([features/search/maintenance-tools.md](../features/search/maintenance-tools.md#get_duplicates-аудит-дублей-проекта)) | MCP, CLI |
| `outline` | `cwd`, `project`, `path`, `levels`, `under` (цепочка заголовков) или `under_line` | заголовки с номерами строк (Т-26) | MCP, CLI |
| `read` | `cwd`, `project`, `path`, необязательные взаимоисключающие `lines` / `under` | исходный `text`, `title`, `revision`; у части — `lines_total` и `range` / `sections` | MCP, CLI |
| `projects` | необязательный `target: {cwd, project?, wt?}` или необязательный `filter` (подстрока без учёта регистра по имени, составному имени и пути; взаимоисключающие, вместе — `invalid_params`); без них — весь реестр | проекты: путь, имя, корень документации, состояние (в том числе `lazy` — индекс ещё не требовали), модули с числом документов и необязательным `title` своего оглавления; необязательные `parent`, `parent_name`, `qualified_name`, `discovered`, `subprojects`, `discovery_truncated`, `worktrees`; рядом с проектами `registry_total` — размер реестра (только в ответе на `filter`) ([подпроекты](../features/projects/subprojects.md), [worktrees](../features/projects/worktrees.md); подпроекты, `discovery_truncated` и `filter` написаны шагами 6.2.1–6.2.3 без проверки сборкой, `worktrees` — проект); MCP передаёт target по умолчанию, весь реестр только по `all=true` | MCP, CLI |
| `session_info` | `cwd` | необязательный проект сессии: имя, модули (`name`, `documents`, `title` оглавления); без моделей и индексации | внутренний запрос MCP при initialize/list_tools |
| `status` | `cwd`, `project` | состояние индекса проекта ([worker.md](worker.md)) | MCP, CLI |
| `reindex` | `cwd`, `project`, `full`, `accept_model_change` | `accepted: true`; без `full` и подтверждения смены модели — независимый `scan` файлов и отдельная очередь обновления; ход виден в `status`, итог проверки в операциях окна | MCP, CLI |
| `register` | `path` | проект после регистрации; выбор корня документации записывается клиентом в `.bars/config.toml` до вызова, параметра корня нет ([projects.md](../features/projects/projects.md)) | CLI |
| `unregister` | `cwd`, `project` | `removed: true` | CLI |
| `session_start` | `cwd`, `source` | `accepted: true`; ответ сразу, проверка идёт в очереди | CLI (хук) |
| `graph_query` | `cwd`, `project`, `request` (`GraphRequest`: `operation`, `scope`, `budgets`, `limit`, `snapshot`, `cursor` и поля операции) | `GraphReply` — узлы, источники, утверждения, evidence, покрытие, усечение | MCP, CLI |
| `graph_search` | параметры `search` плюс `mode` (`hybrid`/`mix`/`graph`), `graph_budgets`, `include_related`, `related_budget` | ответ `search` плюс `requested_mode`, `applied_mode`, `fallback_reason`, `related`, `graph_results` | MCP, CLI |
| `graph_status` | `cwd`, `project` | состояние слоя графа: состояние, снимок, активное задание, число источников, ошибка, версия метода | MCP, CLI |
| `graph_physical` | `cwd`, `project`, `request` (`PhysicalRequest`: `scope`, `limits`, `expanded`, `snapshot`) | `PhysicalGraphSlice` (`physical-v1`) — узлы, рёбра индексами узлов, состояния трёх слоёв, итоги области, усечение; ошибки те же, что у графа, устаревший `snapshot` — `snapshot_expired` | только окно |
| `graph_query_slice` | `cwd`, `project`, `request` (`QuerySliceRequest`: завершённый поиск, область, каналы и хеши источников, глубина, fanout, порог, пределы, снимок) | `QueryGraphSlice` (`query-v1`) — результаты и их окружение из всей области, точный cosine, готовые списки режимов, статусы источников и причины отсутствия на карте, состояния слоёв, fingerprint, усечение и размер всего ответа; устаревший снимок — `snapshot_expired` | только окно |
| `graph_build` | `cwd`, `project` | `accepted: true` и `job_id`; ход виден в `graph_status` | CLI |
| `graph_cancel` | `cwd`, `project`, `job_id` | `cancelled`: задание остановлено или уже не активно | CLI |
| `install` | `client` (`claude-code` / `codex`) | какие файлы изменены, нужно ли одобрить хук в Codex | CLI |
| `uninstall` | `client` | какие файлы изменены | CLI |

`graph_query` и `graph_search` — общие для клиентов; MCP выставляет шесть read-only инструментов графа поверх `graph_query`, операцию выбирает имя инструмента. Записи графа (`graph_build`, `graph_cancel`) доступны CLI и окну, MCP их не получает. Срезы `graph_physical` и `graph_query_slice` доступны только окну; их контракты описывают [проект физического графа](../features/graphrag/physical-graph.md), раздел «Срез, бюджеты и договор данных», и [граф запроса](../features/graphrag/query-graph.md), раздел «Что реализовано в окне и чем отличается от контракта». DTO `GraphRequest`/`GraphReply` принадлежат `bars-graph` и реэкспортируются из `bars-protocol`; их версия (`schema_version`, сейчас 1) не связана с версией протокола. Без `mode` поиск сохраняет обычную форму ответа; `SearchHit` дополнен полем `source_hash` — blake3 полного документа в том же read snapshot, что и фрагмент. Поле передаётся во всех каналах; проверка версии для графа запроса не меняет оценку и порядок поиска. Поля, контракт операций и пределы — [tools.md](../features/graphrag/tools.md#фактический-контракт-v1), устройство слоя — [graph.md](graph.md).

Методы, у которых в колонке «Кто вызывает» нет MCP, для `bars-mcp` не существуют: воркер отвечает на них `-32601`. Первым вызовом на соединении должен быть `hello`; до него любой метод, даже неизвестный, получает `protocol_mismatch`.

## Ограниченные методы чтения с диска

`disk_text` и `disk_links` доступны MCP/CLI через стандартный `Target` (`cwd`, `project?`). Это отдельные DTO: оконные `text_search` и `links_of` остаются `Window`. Контракт параметров, страниц, усечения и ошибок — [два инструмента чтения MCP](../features/agent-tools/mcp-tools.md). `disk_text` имеет независимую отмену и строгую ревизию продолжения; `disk_links` сверяет индекс с диском и проверяет привязку курсора к проекту, документу, направлению и снимку. Модели и готовый embed-индекс не требуются; документы не изменяются. `disk_text` принимает только узкий запрос; широкий отклоняется `invalid_params` с `data.kind = "too_broad"` до обхода диска.

## Ответы методов bars

- `search` — `results` (путь от корня проекта, `module`, `title`, `heading_chain` строкой `Заголовок 1/Заголовок 2`, `sections` — заголовки разделов, приклеенных к чанку после первого, в виде строк `## Заголовок`, поле пропускается, если их нет, `first_line`, `last_line` — строки раздела, `text` — лучшего куска, `score` — `min(1, S)`, `score_kind` — `rerank`, `cosine` или `none`, `signals` — баллы по сигналам, `parts` и `parts_found` — куски раздела), `applied` (число результатов, кандидатов, кандидатов реранка, порог `min_score`, вид оценки, `clamped`, `weights`, `signals`), `understood` (разбор операторов запроса, только если они есть), `indexing`, `pending` и, если rerank не отработал, `rerank_error`; форма — [раздел «Форма ответа поиска»](../features/search/hybrid-search.md#форма-ответа-поиска); если embed-модель не успела загрузиться за предел ожидания, `semantic_error` (выдача только по словам; [ожидание моделей](../features/search/hybrid-search.md#ожидание-моделей)).
- `outline` — `headings`: номер строки, уровень, текст. Строку `7 ## Стадия` из заголовка собирает функция крейта `bars-protocol`, общая для MCP и CLI.
- `read` — `title`, `revision` полного документа и `text` как есть. Без выбора части формат прежний; у части добавляются `lines_total` и `range` (`from`, `to`) либо `sections` (`from`, `to`, `heading_chain`). `text` содержит только выбранные строки, повторяющиеся разделы — в порядке документа. Общий рендерер `bars-protocol` сохраняет исходные номера строк и метки частей для MCP/CLI (Т-20); границы и ошибки — в контракте `bars_read`.
- `status` — поля из таблицы в [worker.md](worker.md) и `path` проекта; время — RFC 3339.
- `projects` и `register` — описание проекта: путь, имя, корень документации, состояние, модули с `name`, `documents` и необязательным `title` собственного оглавления. У `projects` параметр `target` ограничивает чтение одним проектом, `filter` — совпавшими записями реестра; отсутствие обоих сохраняет полный список для CLI. MCP передаёт проект сессии по умолчанию, а полный список запрашивает только через `all=true`. Необязательные поля `ProjectInfo` (в ответе, если непусты; написаны шагом 6.2.1 без проверки сборкой): `parent` (путь), `qualified_name`, `discovered`, `subprojects` (`name`, `path`, `state`, `discovered`, `excluded`), `discovery_truncated` (`true`, пока последний обход проекта оборван пределом глубины или числа записей; обход написан шагом 6.2.2 без проверки сборкой) — [подпроекты](../features/projects/subprojects.md); состояние проекта — и `lazy`, не ошибка; `worktrees` — список worktree владельца с состоянием дельты ([worktrees](../features/projects/worktrees.md)).
- `search` с worktree — в `applied.scope` необязательное поле `SearchScope.worktree` (`name`, `branch`, `state`, `delta_documents`, `pending`); `status` у проекта-владельца получает часть WORKTREES (`WorktreeStatus` на worktree); `tree_read` с `wt` строит дерево worktree, а его `nested` содержит записи вида `worktree` для папок рабочих копий (проект, реализации нет, Т-48; [worktrees](../features/projects/worktrees.md)).

Установкой в клиенты занимается воркер, а не `bars-cli`: так у окна и CLI одна реализация (Т-21), а пути к `bars-mcp` и `bars-cli` воркер знает от своего exe ([features/clients/clients.md](../features/clients/clients.md)).

## Методы окна

История MCP обслуживается отдельными **командами Tauri**, а не вариантами `Call` канала: `mcp_history`, `mcp_history_control`, `mcp_history_legacy`, `mcp_history_diagnostics` и существующая сводка `mcp_statistics`. Их нельзя вызвать как методы MCP/CLI. Входы, результаты и поведение при отказах — [API истории для окна](../features/window/mcp-statistics.md#api-окна-для-этапа-30). Список ниже описывает именно методы `Call`, доступные окну через общий обработчик.

Решение автора по умолчанию (OQ-55, OQ-56) и решение агента, 15-10-2026. Окно вызывает те же обработчики, что канал, но эти методы существуют только для него: по каналу они закрыты всем клиентам, и `bars-mcp`, и `bars-cli` получают на них `-32601`. Метод окна помечается в `bars-protocol` как `Window`; `Call::allowed_for` для него всегда ложь, `Call::window_only` — истина, а обработчик канала отвергает такой вызов ещё раз сам.

| Метод | Параметры | Ответ |
|---|---|---|
| `index_pause` | — | `paused: true`: очередь приостановлена целиком, идущее задание замирает между документами, новые не берутся |
| `index_resume` | — | `paused: false` |
| `index_cancel` | `project` (путь или имя) | `cancelled`: идущее задание прервано между документами или ждущее снято с очереди. Старый индекс цел; проект в `reindex_required` в нём и остаётся — согласие придётся дать снова |
| `activity` | — | `paused`, `running` (проект, вид, приоритет, документов в обходе и обработано, чанков, чанков в секунду, секунд, текущий файл), `queue` (проект, вид, приоритет, число файлов), `problems` (по проектам: файлы с ошибкой и `last_error`, `rebuild_not_applied`); `running` также несёт `embed_device` текущего задания и `attempt`; ошибка файла — `stage`, `attempts`, `previous_version`. `documents` — очередь документов по проектам, где она не пуста: `project`, `total`, счётчики `waiting`/`queued`/`indexing`/`error` и до 200 записей `{path, state, source, due_in, error}` в порядке выполнения ([очередь документов](../features/indexing/document-queue.md)) |
| `index_retry` | `project`, `path` | `retried`: у документа была запись с ошибкой, теперь он снова в очереди |
| `tree` | `cwd`, `project` | `docs_root` и документы из индекса: путь от корня проекта, модуль, `title`, число чанков. Читается без модели, поэтому дерево видно и в состоянии `reindex_required` |

Доставка уведомлений этапа 28 (также только `Window`):

| Метод | Параметры | Ответ |
|---|---|---|
| `operation_events` | необязательные `epoch`, `after` | `epoch`, `cursor`, `gap`, атомарный снимок `active` и завершения `completed` после курсора; без эпохи — начало наблюдения без старых итогов. ID, поколение регистрации, цель, стадия, прогресс, время, собственный отчёт и безопасная ошибка. [Жизненный цикл и пределы](../features/window/notifications.md) |
| `operation_retry` | `epoch`, `id` завершённой ошибки/отмены/частичного результата | `accepted: true` после постановки новой попытки либо объяснение отказа. Worker проверяет поколение и настройки; согласие Т-25 не наследуется |

Остальные методы окна — настройки, модели, ключи, клиенты, корень документации (решение агента, срез 5):

| Метод | Параметры | Ответ |
|---|---|---|
| `settings_get` | — | `file`, `revision` (`размер-хеш blake3` байтов файла, `null` — файла нет), `settings` (таблицы с умолчаниями: `embed`, `rerank`, `search`, `chunking`, `indexing`) и `warnings`. Снимок и ревизия из одного чтения. `indexing.edit_pause_seconds`: целое 0…3600 |
| `settings_save` | `base` — исходные настройки окна, `changes` — объект `таблица.ключ → значение` только изменённых полей, `expected_revision` — строка или `null` | `status: saved/conflict`, `conflicts` (ключ, `base`, `disk`, `mine`), `file`, `revision`, `settings`, `warnings`. При конфликте ни одного поля не пишет. Независимые внешние поля сливаются; TOML проверяется и записывается атомарно. Неизвестное поле или неверное значение — `invalid_params`. Успех ставит проекты на проверку по Т-25. Правила выбора версии — [настройки](../features/settings/settings.md) |
| `models_info` | — | встроенные embed-модели каталога и rerank-модель: размерность, бэкенд (`onnx` или `candle`), предупреждения, `downloaded`, `downloading`, `download_percent` (0…100 или null), `download_error` (безопасная ошибка ручного скачивания или null) и `download_error_reason` (вид ошибки встроенной модели, те же значения, что `data.reason`, или null); папка моделей; `runtime.embed` и `runtime.rerank` с model/source/requested/state/backend/actual/fallback_reason и необязательными precision/revision фактической сессии, без загрузки модели и ожидания инференса |
| `model_download` | `id` | `accepted`; модель скачивается в фоне. Явный итог — в `operation_events`; `downloading` и полный комплект файлов в `models_info` не доказывают готовность сессии |
| `model_retry` | — | `accepted`; снимает паузу перед повтором фоновой загрузки моделей ([встроенные модели](../features/models/local-models.md), «Пауза перед повтором загрузки»); окно зовёт его кнопкой «Повторить» на экране ошибки модели |
| `secret_set` | `name`, `value` | `present: true`; ключ уходит в хранилище ОС ([secrets.md](secrets.md)); значение не попадает ни в лог, ни в `Debug` |
| `secret_status` | `name` | `present`: есть ли запись; сам ключ окну не отдаётся |
| `clients_status` | — | по клиенту (`claude-code`, `codex`): `installed`, `stale` (записи указывают не на текущие `bars-mcp` и `bars-cli`), файлы, которые bars правит, `needs_hook_approval` |
| `instruction` | — | `text` — та же текстовая инструкция для агента, что печатает `bars-cli install --print-instruction`, и `mcp_config` — конфиг MCP-сервера для клиентов, которых bars сам не ставит |
| `set_docs_root` | `project`, `docs_root` | `accepted: true`; запись `docs_root` в `.bars/config.toml`, слежение переезжает, проект ставится на полную проверку |
| `check_all` | — | `accepted: true`; проверка всех проектов сейчас («Проверить сейчас» у пропавшего проекта) |
| `project_delete` | абсолютный `project`, обязательный `confirmed: true` | `removed: true` после остановки заданий и очистки; без подтверждения — `invalid_params`. Только окно; [границы и ошибки](../features/projects/deletion.md) |

## Методы новых тем

`new_topics_list/get` доступны по каналу (`All`), остальные методы — `Window`. Проект — абсолютный путь зарегистрированного проекта, относительный даёт `invalid_params`. Изменяющие методы принимают ожидаемую `revision` темы: несовпадение или удалённая тема — `stale_ref`, запись не происходит. Устройство и исходы — [новые темы](../features/new-topics/design.md).

| Метод | Параметры | Ответ |
|---|---|---|
| `new_topics_list` | `project` | `enabled`, `generation`, `topics` (ID, ревизия, название, статус, даты, число обращений, источники, основания, число вариантов), `store_path`, `losses`, `loss_reason`; проект без `.bars` даёт пустой список, повреждённая или более новая БД — ошибку |
| `new_topics_get` | `project`, `id` | та же строка, `variants_list` (текст, область, `original_params` — JSON параметров первого обращения) и до двух проверок: исходы по вариантам, причины неполноты, метки индекса, найденные фрагменты, `conclusive`; `params.variants` хранит `variant_id`, `applied`, `rerank_error` каждого выполненного поиска |
| `new_topics_mark_missing` | `project`, `request_id`, `query`, `modules`, `files` | `topic_id`, `created`; повтор того же `request_id` идемпотентен; выключенный сбор — `invalid_params` |
| `new_topics_check` | `project`, `id`, `revision`, `expand` | окно и канал MCP/CLI; карточка темы после проверки, спроса не создаёт; решения и удаление остаются только в окне |
| `new_topics_set_status` | `project`, `id`, `revision`, `status` (`resolved` или `open`) | новая `revision` |
| `new_topics_rename` | `project`, `id`, `revision`, `title` (1–300 знаков) | новая `revision` |
| `new_topics_delete` | `project`, `id`, `revision` | `accepted: true` |
| `new_topics_clear` | `project`, обязательный `confirmed: true` | `generation` |
| `new_topics_settings` | `project`, `enabled` | `enabled`, `generation` |
| `links_into` | `project` (абсолютный путь), `paths` (документы удаляемого набора от корня проекта) | `sources` (`path`, `title`, `links` — `line`, `text`): входящие ссылки на набор из документов вне набора; индекс ссылок, модель не нужна; только окно ([индекс ссылок](../features/knowledge-base/link-index.md)) |
| `links_of` | `project` (абсолютный путь), `path` (от корня проекта) | `known`, `outgoing` (`line`, `text`, `raw`, `kind`, `target`, `target_title`, `broken`), `incoming` (`path`, `title`, `lines`); читает индекс ссылок, сверяя его с диском, модель не нужна; только окно ([индекс ссылок](../features/knowledge-base/link-index.md)) |
| `text_search` | `project`, `query`, регистр, regex, путь, `modules`/`files`, пределы; `path`/`offset`/`revision` для порции | документы с вхождениями, UTF-16 диапазоны, усечение, пропуски и время; читает диск без индекса и модели; только окно ([поиск в тексте](../features/search/text-search.md)) |

Методы `audit_list/run/cancel/findings` доступны по каналу (`All`); `audit_schedule/ignore` — `Window`. Проект — абсолютный путь, вид — `broken_links`, `duplicates` или `contradictions` (иначе `invalid_params`). Аудит читает индекс ссылок и индекс чанков, файлы не меняет, модель не загружает. Устройство, очередь и хранилище — [аудиты](../features/knowledge-base/audits.md).

| Метод | Параметры | Ответ |
|---|---|---|
| `audit_list` | `project` | `audits` по виду: `state` (`never`, `queued`, `running`, `complete`, `partial`, `failed`), время последнего запуска и итог, `error`, `result_run_id` и `result_at` последнего полного запуска, `total`, `new_count`, `gone_count`, `overlap_total`, `schedule`, `schedules`; `reindex_required`, `indexing` |
| `audit_run` | `project`, `kind` | `accepted`, `already` (вид уже шёл или ждал), `queued` (ждёт за другим аудитом проекта) |
| `audit_cancel` | `project`, `kind` | `cancelled`: был идущий или ждущий запуск |
| `audit_findings` | `project`, `kind`, `group` (`main`, `overlap`), `include_gone`, `include_ignored` (по умолчанию `false`), `cursor`, `limit` (до 200) | находки последнего полного запуска: `fingerprint`, `ignored`, `standing` (`new`, `known`, `gone`), `tag`, `group`, `reason`, `score`, `subjects` (`path`, `first_line`, `last_line`, `heading`, `role`), `detail`; `total`, `next`; без полного запуска — пустой список |
| `audit_ignore` | `project`, `kind`, `fingerprint`, `ignored` | игнор/возврат находки по отпечатку; `ignored`; неизвестная находка — `invalid_params`; счётчики и обычный список сразу исключают игнорируемую находку |
| `audit_schedule` | `project`, `kind`, `mode` (`off`, `daily`, `weekly`, `on_change` — последнее только у `broken_links`) | `mode`; недопустимый режим — `invalid_params` |

Автозапуск — состояние ОС, а не настройка ([features/distribution/autostart.md](../features/distribution/autostart.md)); методы доступны окну и `bars-cli`, но не `bars-mcp` (аудитория `Cli`), решение агента, срез 7:

| Метод | Параметры | Ответ |
|---|---|---|
| `autostart_get` | — | `supported` (ОС умеет автозапуск), `enabled` (запись есть и ОС её не отключила), `stale` (запись указывает не на текущий `bars`), `command` |
| `autostart_set` | `enabled` | то же, новое состояние; повтор безвреден; на ОС без автозапуска — `invalid_params` |

Вид задания — `rebuild` (полная переиндексация), `check` (проверка проекта), `files` (слежение); приоритет — `explicit`, `watch`, `periodic`. Русские подписи («полная переиндексация по согласию», «проверка при старте») собирает окно.

## Что воркер реализует из методов протокола

Работают методы из таблиц протокола и окна. Подробности — [worker.md](worker.md), «Обработчики методов воркера bars». Пока встроенная embed-модель скачивается, `search` отвечает `model_loading` (OQ-48); загрузку скачанной модели в память он ждёт до предела, дальше отвечает без смысловой части и с полем `semantic_error` ([ожидание моделей](../features/search/hybrid-search.md#ожидание-моделей)); ошибка фоновой загрузки приходит как `provider_error`.

## Версия протокола bars

`protocol` — целое число (сейчас 1), его увеличивает любое несовместимое изменение методов. `bars`, `bars-mcp` и `bars-cli` поставляются вместе, поэтому воркер принимает только свою версию. Чужая версия — ошибка `protocol_mismatch` с текстом «обновите bars»: значит, установлены exe из разных сборок.

### Сверка сборок в hello

Версия протокола и версия крейтов не меняются от коммита к коммиту, поэтому exe из разных сборок с одним протоколом друг от друга не отличить. Для этого воркер в `hello` отдаёт `worker_build` — идентификатор сборки `BUILD_ID` (12 знаков коммита, из которого собран exe; сборочный скрипт `bars-protocol` берёт его у git, без git — `unknown`). Клиент сравнивает его со своим и при расхождении держит предупреждение (`Client::build_warning`), не прерывая соединение: `bars-cli` пишет его в stderr строкой «bars-cli: предупреждение: …», `bars-mcp` — в stderr процесса при открытии соединения. Предупреждения нет, если сборка неизвестна (`unknown`) у любой стороны. Воркер старой сборки, у которого поля нет, считается расхождением. Идентификатор — коммит, а не содержимое дерева: две сборки из одного коммита с разными незафиксированными правками он не различает; cargo пересобирает `bars-protocol`, а с ним все три exe, после каждого нового коммита (`.git/logs/HEAD`).

## Ошибки протокола bars

Стандартные коды JSON-RPC (`-32700`…`-32603`) — для битых сообщений. Ошибки bars несут код, машинное имя в `data.kind` и понятный человеку текст:

| Код | `kind` | Когда |
|---|---|---|
| -32001 | `protocol_mismatch` | нет `hello` или чужая версия протокола |
| -32002 | `project_not_found` | проект не найден по `cwd` или `project`; текст подсказывает `bars-cli register`, а для имени с префиксом главного проекта называет его подпроекты или причину, по которой папка не подпроект ([подпроекты](../features/projects/subprojects.md)) |
| -32003 | `project_unavailable` | папки проекта нет на диске ([projects.md](../features/projects/projects.md)) |
| -32004 | `reindex_required` | индекс построен другой моделью или старой схемой; поиск не работает до согласия пользователя (Т-25) |
| -32005 | `reindex_needs_consent` | `reindex` по проекту в состоянии `reindex_required` без согласия пользователя (Т-25) |
| -32006 | `file_not_found` | нет файла для `read` или `outline` |
| -32007 | `invalid_params` | параметр вне допустимого: неизвестный модуль, `limit` меньше 1; у `disk_text` `data.kind` уточняет вид: `invalid_regex` или `too_broad` (поля `reason`, `limit`, `actual`, `use`); `worktree_unsupported` — `like`, `similar`, `duplicates` и полная переиндексация по worktree (подсказка `wt="."`); `filter` вместе с `project` или `target` в `projects` и `bars_status` |
| -32008 | `provider_error` | модель не ответила; текст без ключей ([secrets.md](secrets.md)). У встроенной модели `data.reason` — `network`, `files_missing`, `out_of_memory` или `runtime` ([экраны ошибок](../features/window/error-states.md), «Виды ошибок встроенной модели»); у внешней `reason` нет |
| -32009 | `model_loading` | встроенная модель ещё скачивается или загружается; в `data.percent` — ход скачивания в процентах, 100 — файлы на месте, модель грузится в память (при 100 `search` ошибку не отдаёт: ждёт загрузку до предела, а с `wait_models: false` отвечает сразу по словам); сообщение предлагает подождать и поясняет, что проверка актуальности индекса (`bars_status` с `refresh: true`) загрузку не ускоряет |
| -32010 | `graph_not_ready` | нужный слой графа не построен или отстаёт; причина и следующий шаг: обычный поиск/чтение, автоматическое обновление после индекса или окно / `bars-cli graph build`; переиндексация не заменяет построение графа. В `mix` вместо ошибки идёт возврат к `hybrid` с причиной |
| -32011 | `graph_capability_unavailable` | запрошенная возможность графа не поддержана (например `change_description` у `impact`, `tokenizer_id` у `context`); сообщение называет поддержанные входы и следующий шаг |
| -32012 | `stale_ref` | ref или endpoint устаревшей ревизии; похожий объект не подбирается |
| -32013 | `snapshot_expired` | снимок прежнего поколения больше недоступен; запрос нужно повторить |
| -32014 | `cursor_mismatch` | курсор не соответствует снимку, области или параметрам запроса |
| -32015 | `cancelled` | операция отменена; это не ответ «совпадений нет» |
| -32016 | `chunk_not_found` | ID чанка для `similar` неизвестен или устарел после обновления документа; чанк нужно найти поиском заново |
| -32017 | `stale_revision` | документ изменился между порциями точного поиска; начать чтение документа заново |
| -32018 | `stale_cursor` | изменился снимок ссылок; начать чтение списка заново |
| -32019 | `graph_timeout` | бюджет графового чтения истёк до загрузки целого проверенного снимка; повторить запрос после прогрева или увеличить `timeout_ms` в разрешённых пределах |
| -32020 | `worktree_not_found` | у проекта нет worktree с таким `wt`; текст и `data` перечисляют worktree проекта (имя, путь, ветка, `locked`) и напоминают `wt="."` (проект, реализации нет, Т-48; [worktrees](../features/projects/worktrees.md)) |

Коды выхода `bars-cli` по ошибкам: `reindex_required` и `reindex_needs_consent` — `3`, недоступный воркер — `2`, остальные — `1`.

## Согласие на переиндексацию после смены модели

Переиндексацию проекта в состоянии `reindex_required` запускает только пользователь (Т-25): кнопкой в окне или командой `bars-cli reindex --accept-model-change`, которая передаёт `accept_model_change: true`. От `bars-mcp` воркер этот параметр не принимает: в вызове от MCP он сбрасывается в `false`, и агент получает `reindex_needs_consent`. Флаг CLI — согласие человека; текстовая инструкция по установке для агентов ([features/clients/clients.md](../features/clients/clients.md)) запрещает агенту передавать его самому.

## Порог агента вне пределов

Порог агента вне пределов из настроек — не ошибка: он прижимается к границе, и в ответе указан применённый порог (Т-24).
