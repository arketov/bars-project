---
title: Находки подключения Ouroboros
---

# Находки подключения Ouroboros

31-10-2026: `razzant/ouroboros` использует массив MCP_SERVERS в общем settings.json и собственный эксклюзивный .lock. Адаптер bars соблюдает этот замок; перенос JSON-формата Claude или хуков SessionStart неверен. Контракт и ограничения — в [подключении клиентов](../../features/clients/clients.md).
