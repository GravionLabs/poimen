; Puts the install directory (poimen.exe, the CLI) on the user PATH and removes it again (#519, #671).
; Per-user PATH under HKCU\Environment, edited with native NSIS registry commands only (no child process:
; an installer that starts PowerShell to write the registry trips Microsoft Defender heuristics).
; ReadRegStr returns the unexpanded value, WriteRegExpandStr keeps REG_EXPAND_SZ. Segments are compared
; case-insensitively (StrCmp), so a reinstall or update adds no duplicate.
; Registers: $R0 Path, $R1 rest to scan, $R2 position, $R3 character, $R4 segment, $R5 result.
!macro NSIS_HOOK_POSTINSTALL
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  ReadRegStr $R0 HKCU "Environment" "Path"
  StrCpy $R1 $R0
  poimen_path_add_next:
    StrCmp $R1 "" poimen_path_add_append
    StrCpy $R2 0
    poimen_path_add_scan:
      StrCpy $R3 $R1 1 $R2
      StrCmp $R3 "" poimen_path_add_segment
      StrCmp $R3 ";" poimen_path_add_segment
      IntOp $R2 $R2 + 1
      Goto poimen_path_add_scan
    poimen_path_add_segment:
      StrCpy $R4 $R1 $R2
      StrCmp $R4 "$INSTDIR" poimen_path_add_done
      IntOp $R2 $R2 + 1
      StrCpy $R1 $R1 "" $R2
      Goto poimen_path_add_next
  poimen_path_add_append:
    StrCmp $R0 "" 0 +3
      StrCpy $R5 "$INSTDIR"
      Goto poimen_path_add_write
    StrCpy $R3 $R0 1 -1
    StrCmp $R3 ";" 0 +3
      StrCpy $R5 "$R0$INSTDIR"
      Goto poimen_path_add_write
    StrCpy $R5 "$R0;$INSTDIR"
  poimen_path_add_write:
    WriteRegExpandStr HKCU "Environment" "Path" "$R5"
  poimen_path_add_done:
  Pop $R5
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  ReadRegStr $R0 HKCU "Environment" "Path"
  StrCmp $R0 "" poimen_path_remove_done
  StrCpy $R1 $R0
  StrCpy $R5 ""
  poimen_path_remove_next:
    StrCmp $R1 "" poimen_path_remove_write
    StrCpy $R2 0
    poimen_path_remove_scan:
      StrCpy $R3 $R1 1 $R2
      StrCmp $R3 "" poimen_path_remove_segment
      StrCmp $R3 ";" poimen_path_remove_segment
      IntOp $R2 $R2 + 1
      Goto poimen_path_remove_scan
    poimen_path_remove_segment:
      StrCpy $R4 $R1 $R2
      IntOp $R2 $R2 + 1
      StrCpy $R1 $R1 "" $R2
      StrCmp $R4 "" poimen_path_remove_next
      StrCmp $R4 "$INSTDIR" poimen_path_remove_next
      StrCmp $R5 "" 0 +3
        StrCpy $R5 $R4
        Goto poimen_path_remove_next
      StrCpy $R5 "$R5;$R4"
      Goto poimen_path_remove_next
  poimen_path_remove_write:
    WriteRegExpandStr HKCU "Environment" "Path" "$R5"
  poimen_path_remove_done:
  Pop $R5
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
