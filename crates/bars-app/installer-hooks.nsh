; Хуки установщика bars (NSIS, Tauri bundler): остановка работающего bars перед заменой файлов,
; замена занятых bars-mcp и bars-cli, запуск после обновления, чистое удаление.
; Описание: docs/features/distribution/installer.md.
;
; Хуки вызываются из шаблона Tauri: PREINSTALL и POSTINSTALL — в установщике, PREUNINSTALL и
; POSTUNINSTALL — в деинсталляторе. Деинсталлятор прежней версии запускается новым установщиком
; на месте (из папки установки), поэтому хуки прежней версии работают при обновлении тоже.

; Работает ли bars.exe: код возврата 0 — работает.
!macro BarsIsRunning
  nsExec::Exec 'cmd /c tasklist /FI "IMAGENAME eq bars.exe" /NH | find /i "bars.exe"'
!macroend

; Просит работающий bars выйти (`bars.exe --quit`: воркер дописывает текущий документ) и ждёт до
; 15 секунд. Если не вышел, его добьёт Restart Manager из шаблона Tauri. `mark` равно 1 при
; обновлении: тогда после установки bars запускается снова.
!macro BarsStopApp mark
  !define BarsId ${__LINE__}
  !insertmacro BarsIsRunning
  Pop $0
  ${If} $0 = 0
    !if "${mark}" == "1"
      WriteRegStr HKCU "${MANUPRODUCTKEY}" "RestartAfterUpdate" "1"
    !endif
    nsExec::Exec '"$INSTDIR\bars.exe" --quit'
    Pop $0
    StrCpy $1 0
    bars_wait_${BarsId}:
      Sleep 500
      IntOp $1 $1 + 1
      !insertmacro BarsIsRunning
      Pop $0
      ${If} $0 = 0
      ${AndIf} $1 < 30
        Goto bars_wait_${BarsId}
      ${EndIf}
  ${EndIf}
  !undef BarsId
!macroend

; bars-mcp и bars-cli могут быть заняты: bars-mcp живёт в сессиях агентов. Занятый файл нельзя
; перезаписать, но можно переименовать: сессия доработает на старом файле, новые сессии берут новый.
!macro BarsSetAsideClient name
  Delete "$INSTDIR\${name}.exe.old"
  ClearErrors
  Delete "$INSTDIR\${name}.exe"
  ${If} ${Errors}
    ClearErrors
    Rename "$INSTDIR\${name}.exe" "$INSTDIR\${name}.exe.old"
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro BarsStopApp 1
  !insertmacro BarsSetAsideClient "bars-mcp"
  !insertmacro BarsSetAsideClient "bars-cli"
!macroend

!macro NSIS_HOOK_POSTINSTALL
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
  ; записи клиентов остаются.
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
    !insertmacro BarsStopApp 0
  ${Else}
    !insertmacro BarsStopApp 1
  ${EndIf}
  !insertmacro BarsSetAsideClient "bars-mcp"
  !insertmacro BarsSetAsideClient "bars-cli"
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Автозапуск (значение bars в Run) шаблон Tauri убирает сам, если это не обновление; здесь —
  ; отметка «Диспетчера задач» и файлы, которые не удалось стереть из-за занятости.
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
    DeleteRegValue HKCU "${MANUPRODUCTKEY}" "RestartAfterUpdate"
  ${EndIf}
  Delete "$INSTDIR\bars-mcp.exe.old"
  Delete "$INSTDIR\bars-cli.exe.old"
  RMDir "$INSTDIR"
!macroend
