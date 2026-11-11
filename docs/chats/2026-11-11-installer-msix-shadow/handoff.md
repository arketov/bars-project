---
title: Передача — установщик и копии bars-mcp/bars-cli в пакете MSIX (ISS-107)
---

# Передача — установщик и копии файлов в пакете MSIX (ISS-107)

Продолжение [передачи шага 1.2.1](../2026-11-05-installer-iss107/handoff.md).

## Сделано

- **Причина «установщик не обновляет exe» найдена.** Сессия Claude Code работала внутри Claude Desktop (пакет MSIX): для её процессов `%LOCALAPPDATA%\bars\bars-mcp.exe` и `bars-cli.exe` — теневые копии от 03-11-2026 в `%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\bars\`, а не настоящие файлы. Установщик автора, запущенный обычным способом, скорее всего обновил настоящую папку. Доказательства и оговорки — [knowledge/installer.md](../../knowledge/installer.md), «Виртуализация AppData».
- **Автор требует от установщика «либо показывать ошибку, либо совершать горячую подмену»** (11-11-2026). Хук `crates/bars-app/installer-hooks.nsh`: `BarsSetAsideIn` (прежний перенос с параметром папки) и `BarsClearPackageShadows` — перебирает `%LOCALAPPDATA%\Packages\*\LocalCache\Local<путь установки>`, стирает или переименовывает копии `bars.exe`, `bars-mcp.exe`, `bars-cli.exe`, `vulkan-1.dll`; в `PREINSTALL` оставшаяся копия exe прерывает установку с её путём, в `PREUNINSTALL` — без ошибки.
- **Документы:** [installer.md](../../features/distribution/installer.md) (абзац «Копии в пакетах MSIX»), [knowledge/installer.md](../../knowledge/installer.md), запись ISS-107 в [issues.md](../../backlog/issues.md) (причина найдена).

## Не сделано

- **Сборки и установки не было** (запрет автора): синтаксис NSIS не проверен компилятором. Первый запуск `scripts/build-installer.ps1` может выявить опечатку в `installer-hooks.nsh`.
- **Хеши настоящих файлов не сверены:** из-под пакета их не видно; автор сверяет из обычного PowerShell: `Get-FileHash $env:LOCALAPPDATA\bars\bars-mcp.exe`.
- Ручная чистка двух теневых файлов в `Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local\bars\` не делалась (автор не стал закрывать Claude): следующая установка новым хуком уберёт их сама (переименует занятые).
- Руководство пользователя [manual/install.md](../../manual/install.md) не правилось: поведение для пользователя прежнее («ничего вручную»).
- Защита от повтора для агентов (`bars-cli` определяет запуск внутри MSIX и предупреждает) не делалась; шаг в PLAN.md не заводился.

## Сложности

- Сначала ошибочно сделан вывод «установщик не заменил файлы и проглотил ошибку»: проверка шла из процесса пакета. Отличие настоящей и «видимой» папок показал `fsutil hardlink list`.

## Дальше

- Собрать установщик **не из оболочки Claude Desktop** (виртуализация создаёт новую тень) и поставить поверх; проверить, что `bars-mcp.exe` из сессии агента новый и в инструментах четыре `bars_*`.
- По решению автора — предупреждение в `bars-cli`/`bars_status` о запуске внутри MSIX.
