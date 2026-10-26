---
title: Парное исследование графа с rerank — журнал
---

# Парное исследование графа с rerank — журнал

- Прежний holdout уже просмотрен; повтор используется для эффекта rerank, не как новая приёмка GR-10. Граф пересобирается из прежних источников текущим кодом.
- Начальный check не видел cargo/Vulkan, затем сборка не нашла libclang; применена штатная MSVC/Vulkan-среда и путь LLVM из build.md.
- Общий check: fmt/clippy/build зелёные, тест local_openai_address_works_without_a_key упал с SQLite vtable constructor failed: chunks_fts; отдельный повтор прошёл, запущен полный повтор без модельного прогона.
