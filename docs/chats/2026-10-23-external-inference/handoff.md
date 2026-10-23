---
title: Другие серверы инференса — результат 5.3
---

# Другие серверы инференса — результат 5.3

## Сделано

5.3 завершён 23-10-2026. Ответ автора OQ-153: «Закрыть 5.3, сохранить запрет на установку». Выбраны llama.cpp и TEI; [исследование](../../knowledge/external-inference.md), [контракт](../../features/models/external-models.md), настройки, руководство, оглавления и changelog обновлены.

Источник `tei` для embed/rerank: нативные тела/ответы, запрет усечения, проверка векторов/индексов/оценок, независимые адреса и ключи. Инструкция Qwen3 только запросу. Исправлена потеря обязательного `model`, совпадающего с builtin default. Прошли 31 HTTP + 29 core settings + 7 worker external + 11 settings_io + 16 UI тестов; svelte-check 0/0, production UI. Логи рядом; общий прогон всех параллельных изменений не заявляется. Ручной review собственных изменений выполнен.

Настоящий llama.cpp b11669/Vulkan, Qwen3 embed/rerank 0.6B Q8_0, RTX 3090: индекс трёх документов, запись/чтение настроек, два правильных лидера, rerank без ошибок — `worker-live.json`. Настоящий TEI 1.9.4/CUDA в WSL2 с multilingual MiniLM embed/rerank прошёл тот же путь; оба адаптера получили ожидаемый 422 на длинном входе вместо усечения — `tei-verified.json`, `tei-info.json`, `tei-backends.json`. Модели и runtime идентифицированы версиями/хешами. Все собственные серверы остановлены, веса на D: сохранены — `cleanup.json`, `tei-cleanup.json`.

Release WebView2 с изолированным FakeProvider: оба TEI-источника, независимые порты, сохранение/чтение через настоящий IPC, размеры 1440/1024/760 — `native.json`. Штатным `scripts/build-installer.ps1` собран NSIS; timestamp/SHA-256/MZ/PE проверены — `installer.json`, `installer-build.log`. Артефакт: `target/external-inference-delivery/installer/bars_0.1.0_x64-setup.exe`, SHA-256 `5A1B74263071F2491F0A65051DBF3E7A58AAB79AACB354073D5F63F48C90634F`.

## Границы результата

Открытой работы по 5.3 нет. По OQ-153 запрет OQ-113 сохранён: установленный bars и рабочий профиль не менялись; приёмка установленной версии и физические жесты не заявляются. Infinity/vLLM — изученные кандидаты API, вне выбранных реализаций. Малый диагностический корпус проверяет совместимость; превосходство качества и сравнительная холодная скорость не измерялись.

## Сложности и воспроизведение

Serena недоступна. Docker Desktop завершился на Inference Manager; без ремонта обошлись существующей WSL2. Обрыв загрузки официального слоя продолжен HTTP Range, полный SHA-256 проверен. Linux runtime сохранён в `/root/Projects/oss/bars-tei-5-3`; запуск — `run-tei-wsl.sh`, клиент — `verify_external_inference tei <embed-url> <embed-model> <rerank-url> <rerank-model> <evidence.json>`. Окно — `workspace_probe <изолированный-каталог> <путь-к-external_inference_probe.js>`.

Автоматическая проверка отклонила удаление собственного Windows portable runtime: `blocked by policy`; `%TEMP%/bars-external-inference-5-3` оставлен. Параллельные dirty-изменения сохранены; общие файлы смешаны с LM Studio и другой работой, отдельный коммит этой сессии не создавался.
