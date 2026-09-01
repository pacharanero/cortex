# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later

!macro PrepareCortexUpgrade
  # The pre-install hook runs before Tauri's standard app shutdown. Close the
  # GUI first so its polling cannot restart a daemon while sidecars are removed.
  # Match the full installed path so another user's process or unrelated
  # same-named software is never terminated.
  # Retain each match before stopping it: Path can become unreadable during
  # termination, which must not be mistaken for the process having exited.
  # Retry discovery and fail closed if a same-named process cannot be safely
  # classified, rather than silently leaving the installed GUI alive.
  nsExec::ExecToLog '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoLogo -NoProfile -NonInteractive -Command "& { $$ErrorActionPreference = [System.Management.Automation.ActionPreference]::Stop; $$target = [IO.Path]::GetFullPath($$args[0]); function Find-CortexGui { $$found = @(); foreach ($$process in @(Get-Process -Name cortex-gui -ErrorAction SilentlyContinue)) { $$path = $$null; for ($$attempt = 0; $$attempt -lt 10 -and $$null -eq $$path; $$attempt++) { try { $$path = [IO.Path]::GetFullPath($$process.Path) } catch {}; if ($$null -eq $$path) { try { $$path = [IO.Path]::GetFullPath($$process.MainModule.FileName) } catch {} }; if ($$null -eq $$path) { try { $$instance = Get-CimInstance -ClassName Win32_Process -Property ProcessId,ExecutablePath | Where-Object { $$_.ProcessId -eq $$process.Id } | Select-Object -First 1; $$path = [IO.Path]::GetFullPath($$instance.ExecutablePath) } catch {} }; if ($$null -eq $$path) { if ($$process.HasExited) { break }; Start-Sleep -Milliseconds 100 } }; if ($$process.HasExited) { continue }; if ($$null -eq $$path) { exit 1 }; if ($$path -eq $$target) { $$found += $$process } }; $$found }; $$processes = @(Find-CortexGui); $$processes | Stop-Process -Force; foreach ($$process in $$processes) { if (-not $$process.WaitForExit(5000)) { exit 1 } }; if (@(Find-CortexGui).Count -ne 0) { exit 1 } }" "$INSTDIR\cortex-gui.exe"'
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
