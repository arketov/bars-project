; Хуки установщика bars (NSIS, Tauri bundler): остановка работающего bars перед заменой файлов,
; перенос занятых exe в сторону, запуск после обновления, чистое удаление.
; Описание: docs/features/distribution/installer.md.
;
; Хуки вызываются из шаблона Tauri: PREINSTALL и POSTINSTALL — в установщике, PREUNINSTALL и
; POSTUNINSTALL — в деинсталляторе. Деинсталлятор прежней версии запускается новым установщиком
; на месте (из папки установки), поэтому хуки прежней версии работают при обновлении тоже.
;
; Файл сохранён в UTF-8 с BOM: без него NSIS прочитал бы русские сообщения MessageBox в кодировке ANSI.
; Метки в макросах получают явные идентификаторы (`id`), а не `__LINE__`: макрос хука вставляет
; их несколько раз в одном месте, и повтор метки не даёт собрать установщик.

; Свежая метка рядом с exe — просьба к клиентам не запускать воркер (`launch.rs`, ISS-107):
; воркер, поднятый посреди установки, занял бы файлы, которые установщик собрался заменить.
; Метка сама «протухает» через 10 минут, если установщик упал. Ошибка записи (папки ещё нет) не страшна.
!macro BarsMarkUpdate
  ClearErrors
  FileOpen $0 "$INSTDIR\update.lock" w
  ${IfNot} ${Errors}
    FileClose $0
  ${EndIf}
  ClearErrors
!macroend

!macro BarsUnmarkUpdate
  Delete "$INSTDIR\update.lock"
!macroend

; Сообщение об ошибке и прерывание установки (Abort в Section: установщик завершается с кодом 2).
; В тихом режиме текст выводится в консоль запустившего процесса, иначе — в окне. Метка снимается:
; после сорванной установки клиенты не должны ждать 10 минут.
!macro BarsFail id text
  !insertmacro BarsUnmarkUpdate
  IfSilent bars_fail_silent_${id} bars_fail_ui_${id}
  bars_fail_silent_${id}:
    System::Call 'kernel32::AttachConsole(i -1)i.r0'
    ${If} $0 != 0
      System::Call 'kernel32::GetStdHandle(i -11)i.r0'
      FileWrite $0 "${text}$\n"
    ${EndIf}
    Goto bars_fail_end_${id}
  bars_fail_ui_${id}:
    MessageBox MB_OK|MB_ICONSTOP "${text}"
  bars_fail_end_${id}:
  Abort
!macroend

; Файл `name` ещё лежит на месте — прежний занят, и его не удалось ни стереть, ни перенести:
; новый файл поверх него не лёг бы, установка осталась бы со старой версией.
!macro BarsRequireFree name id
  ${If} ${FileExists} "$INSTDIR\${name}"
    !insertmacro BarsFail ${id} "Не удалось освободить $INSTDIR\${name}: файл занят. Завершите процессы bars, bars-mcp и bars-cli (закройте сессии агентов) и запустите установку снова."
  ${EndIf}
!macroend

; После копирования файла `name` не оказалось: тихая установка не должна завершаться «успешно»
; с недоставленным файлом.
!macro BarsRequireInstalled name id
  ${IfNot} ${FileExists} "$INSTDIR\${name}"
    !insertmacro BarsFail ${id} "Установка не завершена: файл $INSTDIR\${name} не записан. Запустите установку снова."
  ${EndIf}
!macroend

; Освобождает имя `name` (bars.exe, bars-mcp.exe, bars-cli.exe) в папке установки: стирает, а у
; занятого файла — переименовывает в `name.old`, `name.old1` … `name.old9` (запущенный exe нельзя
; перезаписать, но можно переименовать). В `$3` остаётся новый путь занятого файла, иначе пусто.
; Старые `name.old*` стираются, пока они не заняты: занятые остаются от процессов прежних обновлений.
!macro BarsSetAside name
  StrCpy $3 ""
  Delete "$INSTDIR\${name}.old*"
  ClearErrors
  ${If} ${FileExists} "$INSTDIR\${name}"
    Delete "$INSTDIR\${name}"
    ${If} ${FileExists} "$INSTDIR\${name}"
      ClearErrors
      Rename "$INSTDIR\${name}" "$INSTDIR\${name}.old"
      ${IfNot} ${Errors}
        StrCpy $3 "$INSTDIR\${name}.old"
      ${Else}
        ${For} $2 1 9
          ClearErrors
          Rename "$INSTDIR\${name}" "$INSTDIR\${name}.old$2"
          ${IfNot} ${Errors}
            StrCpy $3 "$INSTDIR\${name}.old$2"
            ${ExitFor}
          ${EndIf}
        ${Next}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ClearErrors
!macroend

; Работает ли bars.exe: код возврата 0 — работает.
!macro BarsIsRunning
  nsExec::Exec 'cmd /c tasklist /FI "IMAGENAME eq bars.exe" /NH | find /i "bars.exe"'
!macroend

; Останавливает bars так, чтобы его не перезапустили клиенты (ISS-107). Прокси `bars-mcp` живут в
; сессиях агентов и запускают воркер заново, едва он вышел (Т-2), поэтому сначала `bars.exe`
; уходит в сторону: прокси прежней версии метку не знают, но запускать им будет нечего, а новый
; `bars.exe` ляжет на его место при копировании. Затем работающий bars просят выйти (`--quit` из
; перенесённого файла: второй запуск передаётся работающему экземпляру, воркер дописывает текущий
; документ) и ждут до 15 секунд; не вышел — завершают по пути к файлу. Перенесённый файл Restart
; Manager из шаблона Tauri уже не находит, поэтому завершать приходится самим. `mark` равно 1 при
; обновлении: тогда после установки bars запускается снова. `id` — уникальная приставка меток.
!macro BarsStopApp mark id
  !insertmacro BarsSetAside "bars.exe"
  !insertmacro BarsIsRunning
  Pop $0
  ${If} $0 = 0
    !if "${mark}" == "1"
      WriteRegStr HKCU "${MANUPRODUCTKEY}" "RestartAfterUpdate" "1"
    !endif
    ${If} $3 == ""
      StrCpy $3 "$INSTDIR\bars.exe"
    ${EndIf}
    nsExec::Exec '"$3" --quit'
    Pop $0
    StrCpy $1 0
    bars_wait_${id}:
      Sleep 500
      IntOp $1 $1 + 1
      !insertmacro BarsIsRunning
      Pop $0
      ${If} $0 = 0
      ${AndIf} $1 < 30
        Goto bars_wait_${id}
      ${EndIf}
  ${EndIf}
  ; Страховка независимо от имени процесса в tasklist: завершаем воркеры из этой папки по пути к
  ; файлу, без фильтра по имени процесса (чужие сборки bars.exe не трогаем; `.Path` у переименованного файла — `bars.exe.old*`) и
  ; проверяем, что их не осталось. Путь передаётся через окружение: кавычки и скобки в нём не ломают
  ; команду. В `$4` — код: 0 — воркеров нет, иначе (1 или error) — воркер остался.
  System::Call 'kernel32::SetEnvironmentVariable(t "BARS_INSTDIR", t "$INSTDIR")'
  nsExec::Exec `powershell -NoProfile -NonInteractive -Command "$$d = $$env:BARS_INSTDIR + '\bars.exe'; $$f = { Get-Process -ErrorAction SilentlyContinue | Where-Object { $$_.Path -and $$_.Path.StartsWith($$d, 'OrdinalIgnoreCase') } }; & $$f | Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Seconds 1; if (@(& $$f).Count -gt 0) { exit 1 }; exit 0"`
  Pop $4
  ; Воркер вышел: перенесённый файл больше не занят и ни для чего не нужен.
  ${If} $4 == "0"
  ${AndIf} $3 != ""
    Delete "$3"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro BarsMarkUpdate
  !insertmacro BarsStopApp 1 "pre"
  ${If} $4 != "0"
    !insertmacro BarsFail "stop" "Не удалось завершить работающий bars из $INSTDIR. Закройте bars (значок в трее — «Выход») и запустите установку снова."
  ${EndIf}
  ; bars-mcp и bars-cli могут быть заняты: bars-mcp живёт в сессиях агентов, убивать его нельзя —
  ; сессия упала бы. Сессия доработает на старом файле, новые сессии берут новый.
  !insertmacro BarsSetAside "bars-mcp.exe"
  !insertmacro BarsSetAside "bars-cli.exe"
  ; Не освободили — не продолжаем: иначе установка молча оставила бы старую версию.
  !insertmacro BarsRequireFree "bars.exe" "free1"
  !insertmacro BarsRequireFree "bars-mcp.exe" "free2"
  !insertmacro BarsRequireFree "bars-cli.exe" "free3"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Файлы легли: проверяем и снимаем метку, дальше клиенты снова запускают воркер.
  !insertmacro BarsRequireInstalled "bars.exe" "done1"
  !insertmacro BarsRequireInstalled "bars-mcp.exe" "done2"
  !insertmacro BarsRequireInstalled "bars-cli.exe" "done3"
  !insertmacro BarsUnmarkUpdate
  ReadRegStr $0 HKCU "${MANUPRODUCTKEY}" "RestartAfterUpdate"
  ${If} $0 == "1"
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "RestartAfterUpdate"
    ; Exec, а не RunAsUser: установщик currentUser не повышен, а запущенный bars наследует окружение.
    Exec '"$INSTDIR\bars.exe" --background'
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Деинсталлятор из папки установки запускает новый установщик, чтобы обновить bars; настоящее
  ; удаление он копирует во временную папку. Первое — обновление: запись автозапуска, ярлыки и
  ; записи клиентов остаются, метка остаётся для установщика, который снимет её сам.
  ${If} "$EXEDIR" == "$INSTDIR"
    StrCpy $UpdateMode 1
  ${EndIf}
  ${If} $UpdateMode <> 1
    ; Записи bars в клиентах не должны указывать на удалённые файлы. Файлы и папки проектов,
    ; индексы .bars и ~/.bars не трогаются.
    nsExec::Exec '"$INSTDIR\bars-cli.exe" uninstall --client claude-code'
    Pop $0
    nsExec::Exec '"$INSTDIR\bars-cli.exe" uninstall --client codex'
    Pop $0
    nsExec::Exec '"$INSTDIR\bars-cli.exe" uninstall --client kimi-code'
    Pop $0
    nsExec::Exec '"$INSTDIR\bars-cli.exe" uninstall --client ouroboros'
    Pop $0
    !insertmacro BarsMarkUpdate
    !insertmacro BarsStopApp 0 "unremove"
  ${Else}
    !insertmacro BarsMarkUpdate
    !insertmacro BarsStopApp 1 "unupdate"
  ${EndIf}
  !insertmacro BarsSetAside "bars-mcp.exe"
  !insertmacro BarsSetAside "bars-cli.exe"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Автозапуск (значение bars в Run) шаблон Tauri убирает сам, если это не обновление; здесь —
  ; отметка «Диспетчера задач», метка и файлы, которые не удалось стереть из-за занятости.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "RestartAfterUpdate"
    !insertmacro BarsUnmarkUpdate
  ${EndIf}
  Delete "$INSTDIR\bars.exe.old*"
  Delete "$INSTDIR\bars-mcp.exe.old*"
  Delete "$INSTDIR\bars-cli.exe.old*"
  RMDir "$INSTDIR"
!macroend
