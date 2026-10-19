# Находки сессии: «Новые темы», внутренний API окна (шаг 20, P3)

- Сборка: из `bars-project/target` CMake падает на вложенном shader generator Vulkan (кириллица в пути). Рабочий набор — `CARGO_TARGET_DIR=C:/bars-build`, `CMAKE_GENERATOR=Ninja`, `CMAKE_BUILD_PARALLEL_LEVEL=1`, `VULKAN_SDK`, `LIBCLANG_PATH` ([build.md](../../knowledge/build.md)).
- `cargo fmt` по трём крейтам переписал чужие `guard.rs` и `bench_duplicates.rs`; откатил `git checkout`, форматировать только свои файлы.
- Автор (19-10-2026): в этой сессии только P3, ручное объединение/разъединение тем отложено (OQ-102).
