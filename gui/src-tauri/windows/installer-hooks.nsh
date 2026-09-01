# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later

!macro PrepareCortexUpgrade
  # The pre-install hook runs before Tauri's standard app shutdown. Close the
  # GUI first so its polling cannot restart a daemon while sidecars are removed.
  # Match the full installed path so another user's process or unrelated
  # same-named software is never terminated.
  # Retain each match before stopping it: Path can become unreadable during
  # termination, which must not be mistaken for the process having exited.
  # Use CIM so 32-bit PowerShell can classify the 64-bit GUI, and fail closed
  # if a same-named process has no readable executable path.
  # Keep the command below NSIS's default 1,024-character string limit.
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -Command "& { $$t = [IO.Path]::GetFullPath($$args[0]); $$n = [IO.Path]::GetFileName($$t); function Find-CortexGui { $$f = @(); foreach ($$i in @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object Name -eq $$n)) { if ([string]::IsNullOrEmpty($$i.ExecutablePath)) { exit 1 }; if ([IO.Path]::GetFullPath($$i.ExecutablePath) -eq $$t) { $$p = Get-Process -Id $$i.ProcessId -ErrorAction SilentlyContinue; if ($$null -ne $$p) { $$f += $$p } } }; $$f }; $$ps = @(Find-CortexGui); $$ps | Stop-Process -Force -ErrorAction Stop; foreach ($$p in $$ps) { if (-not $$p.WaitForExit(5000)) { exit 1 } }; if (@(Find-CortexGui).Count) { exit 1 } }" "$INSTDIR\cortex-gui.exe"'
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
