---
title: GraphRAG — renderer и макет 18-10-2026
---

# GraphRAG — renderer и макет 18-10-2026

Изолированный headless Chrome 128 на Windows, Node 24.16.0, WebGL ANGLE/RTX 3090. Никакого доступа к установленному bars или профилям клиентов. Это проверка браузерного renderer/фикстур, не native Tauri и не Mac.

- `renderer.json` — 18 прогонов: одинаковая спираль, дерево и соседние связи; 250/1000/2000 узлов, три повтора каждой библиотеки.
- `mockup.json` — проверки видов, шести операций, состояний, scope, клавиатуры, источника, обеих тем/760 px; данные явно тестовые.
- `component.json` — проверки настоящего Svelte-компонента/Cytoscape на записанных Rust DTO: все виды/операции, quote/Escape, pin/unpin, destroy и поиск mix/graph; backend здесь фикстурный.
- `validation.json` — итог проверенного среза: Rust/IPC/proxies, UI/сборка, работающий установленный bars; отдельно непроведённые приёмки и недоступный bars_rag_search.
- `component-light-1440.png`, `component-light-760.png`, `component-dark-1440.png`, `component-dark-760.png` — настоящий компонент на тех же тестовых документах, отдельно от макета P1.
- `light-1440.png`, `light-760.png`, `dark-1440.png`, `dark-760.png` — макет P1.

## Выбор renderer для P3

| Узлы/рёбра | Cytoscape первый кадр, среднее мс | Cytoscape среднее p95 кадров, мс | Sigma первый кадр, среднее мс | Sigma среднее p95 кадров, мс |
|---|---:|---:|---:|---:|
| 250 / 490 | 243,5 | 14,5 | 2360,7 | 8,4 |
| 1000 / 1990 | 644,0 | 54,8 | 212,8 | 8,2 |
| 2000 / 3990 | 634,4 | 51,2 | 135,7 | 7,1 |

Sigma 250: холодный первый запуск 6744,4 мс, затем 167,1/170,5 мс. Не прятать этот запуск в среднем прогретых измерений. Sigma быстрее при больших картах; Cytoscape предсказуемее при первом открытии малого ограниченного графа. Выбор P3: Cytoscape 3.34.4, preset/ограниченная раскладка, доступный список и явное усечение. Стрелки/разные стили predicates, pin/unpin, остановка layout и destroy поддерживаются без собственного WebGL program. При 2000 узлах плавность ниже; native-приёмка остаётся открытой. Во всех 18 прогонах после destroy/kill осталось 0 canvas.

Пакеты проверены 18-10-2026: Cytoscape 3.34.4 и Sigma 4.0.3 обновлены 15-10-2026, MIT; Graphology 0.26.0 — MIT. Точный lock спайка хранится в `spikes/graph-renderer/`. [Cytoscape API/performance](https://js.cytoscape.org/), [Sigma rendering](https://www.sigmajs.org/concepts/rendering/), [Sigma lifecycle](https://www.sigmajs.org/concepts/lifecycle/). Производственные зависимости не включают Playwright/Sigma/Graphology.

## Границы доказательств

Проверены исходники макета и четыре снимка: список/карта переключаются, длинные имена читаются в инспекторе; 760 px складывает инспектор под карту. Headless обнаружил ошибку SVG hidden: исправлена toggleAttribute, регрессия добавлена. Отдельная проверка настоящего окна, source navigation к реальному файлу и recovery WebView ещё обязательны. Метрики аппаратно зависимы; не переносить их на пользовательские проекты или macOS.
