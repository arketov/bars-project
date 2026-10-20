---
title: Доказательства исследования физики графа 21-10-2026
---

# Доказательства исследования физики графа 21-10-2026

[← Итог сессии](../handoff.md) · [Метод и выводы](../../../knowledge/graph-render.md)

Успешный исследовательский прогон; headless Chromium, не продуктовая приёмка.

| Файл | Когда читать |
|---|---|
| [measurement.json](measurement.json) | 36 измерений физики, 72 renderer, веса, worker stop/heartbeat, версии/лицензии/даты, success=true |
| [manifests.json](manifests.json) | Ревизии исходных файлов, состояние partial, SHA-256 и размер проекций |
| [snapshots.json.gz](snapshots.json.gz) | Точные нагрузочные проекции для повторения без доступа к индексам; тексты evidence не экспортировались |
| [captures.json](captures.json) | Восемь снимков: два проекта × CoSE/d3/FA2 и отдельная настройка d3, общий renderer Sigma |
| [verification.json](verification.json) | Сверка SHA-256 проекций, полноты измерений, концов рёбер, веса, worker, lifecycle и наличия снимков |
| [tuning.json](tuning.json) | Отдельный опыт с нормализацией пружин по степени, collision и слабым центром; не часть основного сравнения |
| [bars-project-cose.png](bars-project-cose.png) | CoSE на bars-project |
| [bars-project-d3.png](bars-project-d3.png) | d3 на bars-project |
| [bars-project-fa2.png](bars-project-fa2.png) | FA2 на bars-project |
| [overdrive-bots-cose.png](overdrive-bots-cose.png) | CoSE на overdrive-bots |
| [overdrive-bots-d3.png](overdrive-bots-d3.png) | d3 на overdrive-bots |
| [overdrive-bots-fa2.png](overdrive-bots-fa2.png) | FA2 на overdrive-bots |
| [bars-project-d3-tuned.png](bars-project-d3-tuned.png) | Настроенный d3 на bars-project |
| [overdrive-bots-d3-tuned.png](overdrive-bots-d3-tuned.png) | Настроенный d3 на overdrive-bots |
| [bars-project-pin-stop.json](bars-project-pin-stop.json) | Pin, stop, release simulation на bars-project |
| [overdrive-bots-pin-stop.json](overdrive-bots-pin-stop.json) | Pin, stop, release simulation на overdrive-bots |
| [stand.png](stand.png) | Пустой контейнер после destroy последнего renderer; не вид графа |
