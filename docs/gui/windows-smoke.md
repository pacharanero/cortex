# Windows tester-preview smoke

Windows support is under active development. The named-pipe daemon boundary, detached startup, sibling-helper discovery, NSIS package and native Windows test job are implemented, but Windows remains provisional until this runbook passes against real Cortex hardware. The preferred first pass is a disposable QEMU/KVM Windows VM; repeat the same package and device checks on a native Windows machine before calling the host supported.

This smoke is intentionally non-persistent. It reads device state, exercises daemon and GUI lifecycle, and may switch the active Quad scene before returning it. It does not save or delete presets. Mute or disconnect outputs before any scene change.

## Record without publishing device data

Keep a short local result containing:

- Native Windows or QEMU/KVM, Windows edition and build, and QEMU/libvirt versions where applicable.
- Installer filename and SHA-256, installed `cortex` version, device model, and CorOS/application firmware versions.
- Pass/fail and timings for install, direct read, held-session read, GUI launch, reconnect, daemon expiry, upgrade and uninstall.
- Sanitised process and pipe presence. Replace the 16-character user-scope hash in pipe names with `<redacted-user-scope>`.

Do not publish full JSON output, daemon logs or USB captures. They can contain serial numbers, MAC addresses, device names, preset/Capture/IR names and other owner-specific data. Do not add any capture to the repository.

## Get the package

Use the Windows x86_64 artifact from the native release-preview job or a published tester-preview release. A release download includes `SHA256SUMS`; a pull-request artifact does not become trusted merely because it downloaded successfully, so retain the workflow run URL and commit SHA with that result.

For a release package, verify the checksum in PowerShell before accepting the unsigned SmartScreen prompt:

```powershell
$Package = Get-Item .\Cortex_*_x64-setup.exe
if (@($Package).Count -ne 1) { throw 'Expected exactly one Cortex installer' }

$Pattern = [regex]::Escape($Package.Name) + '$'
$Expected = ((Select-String .\SHA256SUMS -Pattern $Pattern).Line -split '\s+')[0]
$Actual = (Get-FileHash $Package.FullName -Algorithm SHA256).Hash
if ($Actual -ne $Expected) { throw 'Installer SHA-256 mismatch' }
```

The NSIS package is current-user-only and unsigned. SmartScreen may warn; do not bypass a checksum mismatch. The default install directory is `%LOCALAPPDATA%\Cortex`, containing `cortex-gui.exe`, the sibling `cortex.exe` session helper and `uninstall.exe`.

## Connect the device

Only one effective editor may own the HID interface. Quit Neural DSP Cortex Control, stop every host or guest `cortex` session, and connect only the product being tested. The Quad is `152a:880a`; the Nano is `152a:88e7`. Windows should use its normal HID driver. Do not replace it with WinUSB through Zadig.

### Native Windows

Connect the unit directly and confirm it appears in Device Manager before running the package. A native pass is the final host evidence because it removes QEMU USB reset and scheduling behavior from the result.

### QEMU/KVM with libvirt

On the Linux host, stop local sessions before assigning the complete composite USB device to the guest:

```sh
cortex session stop --device quad || true
cortex session stop --device nano || true
lsusb -d 152a:880a # Quad
lsusb -d 152a:88e7 # Nano
```

Create a temporary XML file for one model. Quad:

```sh
USB_XML="$(mktemp --suffix=.xml)"
cat >"$USB_XML" <<'XML'
<hostdev mode='subsystem' type='usb'>
  <source>
    <vendor id='0x152a'/>
    <product id='0x880a'/>
  </source>
</hostdev>
XML
```

For Nano, use product `0x88e7`. Start the guest and attach the file only to the live VM:

```sh
virsh -c qemu:///system start win11
virsh -c qemu:///system attach-device win11 "$USB_XML" --live
virt-viewer --connect qemu:///system win11 &
```

Use the actual VM name in place of `win11`. Do not add `--config` or `--persistent` for this smoke. Detach before shutting down the guest or returning the device to Linux:

```sh
virsh -c qemu:///system detach-device win11 "$USB_XML" --live
rm -f "$USB_XML"
```

Vendor/product matching is sufficient when only one unit of that model is attached. If two identical units are present, select the intended host bus/device or physical port explicitly. Bus/device numbers change after reconnection; a physical port path is stable.

For a direct QEMU command line, add one xHCI controller and one host device to the existing VM command. Quad:

```text
-device qemu-xhci,id=xhci -device usb-host,bus=xhci.0,vendorid=0x152a,productid=0x880a
```

Use `productid=0x88e7` for Nano. Passthrough assigns the whole USB device, including audio, MIDI and HID interface 5. The Linux host still sees its transfers through `usbmon`, but capturing is not part of this smoke and raw captures must remain private. QEMU connections on the development machine have historically dropped during long sessions, so use short checkpointed runs and repeat a failure on native Windows before attributing it to the client.

## Install and package boundary

Run the installer interactively without elevation, then initialise these paths in PowerShell:

```powershell
$InstallDir = Join-Path $env:LOCALAPPDATA 'Cortex'
$Cli = Join-Path $InstallDir 'cortex.exe'
$Gui = Join-Path $InstallDir 'cortex-gui.exe'

foreach ($Path in @($Cli, $Gui, (Join-Path $InstallDir 'uninstall.exe'))) {
    if (-not (Test-Path $Path)) { throw "Missing installed file: $Path" }
}
& $Cli --version
Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Cortex' |
    Select-Object DisplayName, DisplayVersion, InstallLocation
```

Expected:

- Installation succeeds as the current user and all three executables exist.
- `cortex --version` matches the package version.
- Add/Remove Programs records `%LOCALAPPDATA%\Cortex` as the install location.

## No-device behavior

Before attaching USB, or with the device detached from the VM:

```powershell
& $Cli session status --device quad --format json
& $Cli session status --device nano --format json
Start-Process $Gui
```

Both status commands should return `{"running":false}`. The GUI must show the real unavailable-device state, never fictional fixture content or a fixture banner. Close it and confirm a failed start did not leave either process behind:

```powershell
Get-Process cortex,cortex-gui -ErrorAction SilentlyContinue
```

## Quad read and held-session smoke

Attach the Quad, keep Cortex Control closed, and run:

```powershell
$DirectVersionJson = & $Cli device version --format json
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($DirectVersionJson)) {
    throw 'Direct Quad version read failed'
}
$DirectVersion = $DirectVersionJson | ConvertFrom-Json
$DirectVersion | Select-Object device_type, coros_version, app_firmware

& $Cli session start --device quad

$Status = & $Cli session status --device quad --format json | ConvertFrom-Json
$Status | Select-Object auto_managed, idle_timeout_seconds
$Status.device | Select-Object state, coros_version
$Status.cache | Select-Object phase, generation, revision
if ($Status.device.state -ne 'connected' -or $Status.cache.phase -ne 'live') {
    throw 'Quad held session did not become live'
}

$HeldVersionJson = & $Cli device version --format json
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($HeldVersionJson)) {
    throw 'Daemon-routed Quad version read failed'
}
$HeldVersion = $HeldVersionJson | ConvertFrom-Json
$HeldVersion | Select-Object device_type, coros_version, app_firmware

$Grid = & $Cli grid show --format json | ConvertFrom-Json
if ($null -eq $Grid) { throw 'Quad live-grid read returned no value' }
'Quad live-grid read passed; full grid intentionally not printed'

& $Cli session stop --device quad
& $Cli session status --device quad --format json
```

Expected: one bounded direct version read before daemon startup, then a connected device, live cache, daemon-routed version/grid reads, clean stop and `{"running":false}`. The complete version and grid objects remain local because they can identify the unit and its owner.

## Nano read and held-session smoke

Detach the Quad, attach the Nano, and disconnect phone/tablet Bluetooth control before starting:

```powershell
& $Cli session start --device nano

$Status = & $Cli session status --device nano --format json | ConvertFrom-Json
$State = & $Cli nano state --format json | ConvertFrom-Json
$Status.device | Select-Object state, coros_version
$Status.cache | Select-Object phase, generation, revision
if ($Status.device.state -ne 'connected') { throw 'Nano session did not connect' }
if (@($State.slots).Count -ne 8) { throw 'Nano state did not contain eight fixed roles' }
'Nano eight-role state read passed; full state intentionally not printed'

& $Cli session stop --device nano
& $Cli session status --device nano --format json
```

Expected: connected state, eight fixed roles and clean shutdown. An immediate typed `Device is busy!` error means another Bluetooth editor owns the Nano; record it as an ownership conflict, disconnect Bluetooth control and retry.

## GUI and daemon lifecycle

Run this section separately for each available product. For Quad, start an explicit daemon and launch the installed GUI:

```powershell
& $Cli session start --device quad
Start-Process $Gui
```

Confirm the GUI reaches live daemon-backed state and agrees with the unit. On Quad, note the active scene, switch once in the GUI, confirm the unit follows, and switch back. This is non-persistent but can be audible. Close the GUI, wait, and prove that an explicitly started daemon survives GUI closure:

```powershell
Start-Sleep -Seconds 70
& $Cli session status --device quad --format json
& $Cli session stop --device quad
```

Next prove GUI-created daemon expiry:

1. Start with the product session stopped.
2. Launch the GUI and wait for live state.
3. Query status once and confirm `auto_managed` is `true` and `idle_timeout_seconds` is `60`.
4. Close the GUI. Do not poll status during the idle interval because a completed status request resets the timer.
5. Wait 70 seconds, query once, and expect `{"running":false}`.

To test reconnect, leave the GUI open and record only `cache.generation`. Physically unplug/replug on native Windows, or detach/reattach the live libvirt USB XML. The GUI must enter reconnecting state, hide stale live data, then return with a greater generation and state matching the unit. A QEMU-only reconnect failure must be repeated natively before becoming a Windows client finding.

## Named-pipe isolation check

While a session runs, list only redacted endpoint names:

```powershell
[System.IO.Directory]::GetFiles('\\.\pipe\') |
    ForEach-Object { [System.IO.Path]::GetFileName($_) } |
    Where-Object { $_ -match '^cortex(-claim)?-[0-9a-f]{16}(-nano)?$' } |
    ForEach-Object { $_ -replace '[0-9a-f]{16}', '<redacted-user-scope>' }
```

Confirm communication and claim pipes exist only while expected. Do not publish the real scope hash. Cross-account pre-claim denial remains a tracked availability limitation under CLI-004.12; the current-account ACL, anonymous client security quality-of-service and server-token check prevent a different account from reading commands or supplying accepted responses.

## Upgrade and uninstall

A real upgrade requires two different package versions. A same-version reinstall exercises only NSIS maintenance behavior.

1. Leave the GUI and one product session running.
2. Run the newer installer interactively.
3. Confirm it closes the GUI, stops both product sessions and replaces the old sibling helper. It must abort rather than continue if the old helper cannot be removed.
4. Run `& $Cli --version` and confirm it reports the newer package version.
5. Confirm neither old daemon survives with an incompatible protocol version, then repeat one held-session read.

After retaining the smoke result, uninstall:

```powershell
& "$env:LOCALAPPDATA\Cortex\uninstall.exe"
```

Confirm `%LOCALAPPDATA%\Cortex`, the Start Menu shortcut and the uninstall registry entry are removed. Tauri application data may remain unless **Delete app data** was selected; that is independent of package removal.

## Pass boundary

A Windows QEMU pass establishes that the package, named-pipe daemon, GUI boundary and USB transport work through virtualised Windows. A native Windows pass establishes the host without QEMU in the path. Neither changes protocol evidence already measured on Linux, and neither makes macOS supported. Record failures at the narrowest boundary: package, process lifecycle, IPC, HID open, handshake, read, GUI state, reconnect, upgrade or uninstall.
