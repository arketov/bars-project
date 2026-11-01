---
title: Журнал сессии — клиент Kimi Code
---

# Журнал: клиент Kimi Code

## 2026-10-31

- Автор закрыл вопросы до ответа: «сделай как предложено» — полный паритет клиента, skills в свою папку, без permission-правила и без гейта (OQ-219…222 закрыты в QUESTIONS.md).
- `cargo test -p bars-worker` собирает serde без фичи `rc` и падает компиляцией теста `graph_cache.rs`; штатно тесты гоняются workspace-wide (`scripts/check.ps1`), там feature-унификация другая. Не дефект кода.
- В оболочке сессии нет окружения сборки: нужны `VULKAN_SDK=C:/tools/VulkanSDK/1.4.363.0` (+`LIB`), `LIBCLANG_PATH=C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/VC/Tools/Llvm/x64/bin`, иначе падают build-скрипты llama-cpp-sys-2/bindgen (см. build.md).
- На машине автора `~/.kimi-code/mcp.json` уже содержит ручную запись `bars` без `type/args/env` и без хука: фоновое автообновление доведёт её до канонического вида и добавит хук SessionStart при первом запуске новой сборки.
