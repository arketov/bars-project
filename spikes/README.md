# Спайки bars

Выбрасываемый код для проверки технических рисков стека. Это не код bars: после записи результатов в `docs/knowledge/` папка удаляется.

Сборка — вне OneDrive, чтобы не синхронизировать гигабайты промежуточных файлов:

```bash
export CARGO_TARGET_DIR="$LOCALAPPDATA/bars-spikes/target"
cargo build --release --manifest-path spikes/search/Cargo.toml
```

## search

Нарезка, индекс SQLite (FTS5 + векторы BLOB), гибридный поиск, rerank. Модели качаются в `~/.bars/models/`.

```bash
spike-search index docs "$LOCALAPPDATA/bars-spikes/data/bars.sqlite"
spike-search query "$LOCALAPPDATA/bars-spikes/data/bars.sqlite" spikes/search/queries.txt --rerank
spike-search bench "$LOCALAPPDATA/bars-spikes/data/bars.sqlite" 5000,50000
```

## detach

Заглушка stdio-MCP: при `initialize` запускает отвязанный дочерний процесс, который 15 минут пишет пульс в `%LOCALAPPDATA%\bars-spikes\detach\heartbeat.log`. Если пульс идёт после закрытия сессии агента, то воркер, запущенный из `bars-mcp`, переживает сессию.
