# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later

!macro StopCortexSessions
  # The pre-install hook runs before Tauri's standard app shutdown. Close the
  # GUI first so its polling cannot restart a daemon while sidecars are removed.
  nsExec::ExecToLog '"$SYSDIR\taskkill.exe" /F /T /IM cortex-gui.exe'
  Pop $0
  IfFileExists "$INSTDIR\cortex.exe" 0 cortex_force_stop
  nsExec::ExecToLog '"$INSTDIR\cortex.exe" session stop --device quad'
  Pop $0
  nsExec::ExecToLog '"$INSTDIR\cortex.exe" session stop --device nano'
  Pop $0
  # The daemon force-exits after a three-second shutdown grace period.
  Sleep 4000
  cortex_force_stop:
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -Command "& { Get-Process -Name cortex,cortex-gui -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 250; if (Get-Process -Name cortex,cortex-gui -ErrorAction SilentlyContinue) { exit 1 } }"'
  Pop $0
  StrCmp $0 "0" cortex_processes_stopped
  MessageBox MB_OK|MB_ICONSTOP "Cortex could not close its running processes. Close them manually, then run the installer again."
  Abort
  cortex_processes_stopped:
  # Delete explicitly so an upgrade cannot retain an unversioned stale sidecar.
  Delete "$INSTDIR\cortex.exe"
  IfFileExists "$INSTDIR\cortex.exe" 0 cortex_sessions_stopped
  MessageBox MB_OK|MB_ICONSTOP "Cortex could not remove its old session helper. Close it manually, then run the installer again."
  Abort
  cortex_sessions_stopped:
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro StopCortexSessions
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro StopCortexSessions
!macroend
