; Puts the install directory (poimen.exe, the CLI) on the user PATH and removes it again (#519).
; Per-user PATH under HKCU\Environment, written through PowerShell so the entry is added once, kept as REG_EXPAND_SZ.
!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec `powershell -NoProfile -NonInteractive -Command "$$d='$INSTDIR'; $$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$$true); $$p=$$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); if(($$p -split ';') -notcontains $$d){ $$k.SetValue('Path',(($$p.TrimEnd(';'),$$d) -join ';').TrimStart(';'),[Microsoft.Win32.RegistryValueKind]::ExpandString) }"`
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  nsExec::Exec `powershell -NoProfile -NonInteractive -Command "$$d='$INSTDIR'; $$k=[Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment',$$true); $$p=$$k.GetValue('Path','',[Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames); $$n=(($$p -split ';') | Where-Object { $$_ -and $$_ -ne $$d }) -join ';'; $$k.SetValue('Path',$$n,[Microsoft.Win32.RegistryValueKind]::ExpandString)"`
  Pop $0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend
