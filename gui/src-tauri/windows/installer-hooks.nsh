# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later

!macro PrepareCortexUpgrade
  # The pre-install hook runs before Tauri's standard app shutdown. Close the
  # GUI first so its polling cannot restart a daemon while sidecars are removed.
  # Match the full installed path so another user's process or unrelated
  # same-named software is never terminated.
  # Retain each match before stopping it: Path can become unreadable during
  # termination, which must not be mistaken for the process having exited.
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -Command "& { $$target = [IO.Path]::GetFullPath($$args[0]); function Find-CortexGui { @(Get-Process -Name cortex-gui -ErrorAction SilentlyContinue | Where-Object { try { [IO.Path]::GetFullPath($$_.Path) -eq $$target } catch { $$false } }) }; $$processes = Find-CortexGui; $$processes | Stop-Process -Force -ErrorAction SilentlyContinue; foreach ($$process in $$processes) { if (-not $$process.WaitForExit(5000)) { exit 1 } }; if (@(Find-CortexGui).Count -ne 0) { exit 1 } }" "$INSTDIR\cortex-gui.exe"'
  Pop $0
  StrCmp $0 "0" cortex_gui_stopped
  MessageBox MB_OK|MB_ICONSTOP "Cortex could not close its installed GUI. Close it manually, then run the installer again." /SD IDOK
  Abort
  cortex_gui_stopped:
  # Delete explicitly so an upgrade cannot retain an unversioned stale sidecar.
  # Never ask a daemon to stop implicitly: its explicit stop watchdog may end
  # an in-flight operation. Any daemon or direct command keeps this file locked,
  # so fail closed and let the operator stop it at a safe point.
  Delete "$INSTDIR\cortex.exe"
  IfFileExists "$INSTDIR\cortex.exe" 0 cortex_helper_removed
  MessageBox MB_OK|MB_ICONSTOP "Cortex could not remove its old session helper. Wait for direct operations to finish, stop both Cortex sessions manually, then run the installer again." /SD IDOK
  Abort
  cortex_helper_removed:
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro PrepareCortexUpgrade
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro PrepareCortexUpgrade
!macroend
