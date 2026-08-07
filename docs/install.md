# Install

## Current Preview Requirements

- **Linux.** The leaf crate is portable, but the installed CLI/MCP host boundary is currently supported only on Linux. Windows named pipes and native Windows/macOS lifecycle and hardware paths remain unimplemented or unverified.
- **Rust** (stable). Install via [rustup](https://rustup.rs/).
- **A C build toolchain, `pkg-config`, libudev development files, and `protoc`.** The CLI's hidapi backend needs the native USB/HID prerequisites; the build script needs the Protocol Buffers compiler.
- A **Quad Cortex**, connected by USB and powered on.

=== "Arch / CachyOS"

    ```sh
    sudo pacman -S --needed base-devel pkgconf protobuf systemd
    ```

=== "Debian / Ubuntu"

    ```sh
    sudo apt install build-essential pkg-config libudev-dev protobuf-compiler
    ```

=== "Fedora"

    ```sh
    sudo dnf install gcc pkgconf-pkg-config systemd-devel protobuf-compiler
    ```

## 1. Grant access to the device

This is the step people get stuck on, and the symptom is confusing: the tool builds and installs perfectly, then reports that it cannot find a device that is plainly plugged in.

The Quad Cortex appears as a USB HID device, and `/dev/hidraw*` nodes are root-only by default. One udev rule fixes it:

```sh
echo 'KERNEL=="hidraw*", ATTRS{idVendor}=="152a", ATTRS{idProduct}=="880a", MODE="0660", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/70-quadcortex.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=hidraw
```

Then **unplug and replug the unit**.

`TAG+="uaccess"` grants access to whoever is logged in at the seat, via systemd-logind. That is preferable to adding yourself to a group: the permission follows the login session rather than being permanent, and there is no group to create.

??? question "How do I check it worked?"

    Find the node and look for the `+` that indicates an ACL:

    ```sh
    lsusb -d 152a:880a
    ls -l /dev/hidraw*
    ```

    You want a line like `crw-rw----+ 1 root root ... /dev/hidraw7`. Without the rule it reads `crw------- 1 root root`.

## 2. Quit Cortex Control

The protocol requires one effective HID owner, but the OS/device does not enforce it safely: a second process may open successfully and silently break the first owner's next request. If Neural DSP's Cortex Control is running, including in a VM with USB passthrough, quit it first.

The same applies in reverse: while `cortex` holds a session, Cortex Control will not connect.

## 3. Install the preview

```sh
git clone https://github.com/pacharanero/cortex
cd cortex
s/install
```

`s/install` currently builds and installs both `cortex` and `cortex-mcp` from the checkout. It exists because the obvious command does not work here: this repo is a Cargo workspace whose root manifest has no `[package]`, so `cargo install --path .` fails. Prebuilt Linux release archives and a checksum-verifying installer are the next distribution milestone.

It also checks for the udev rule and prints the fix if it is missing.

??? note "Options"

    ```sh
    s/install --force     # reinstall over an existing copy
    s/install --debug     # faster build, slower runtime
    s/install --root ~/.local
    s/install --cli-only  # omit cortex-mcp
    ```

    Any other flags are passed through to `cargo install`.

    `--mcp` remains accepted as a compatibility alias, but MCP is installed by default.

## 4. Shell completions

```sh
cortex completions install
```

This detects your shell, writes the completion file to the conventional place, and prints any one-time setup still needed. It never edits your shell startup files.

For zsh it writes `~/.zfunc/_cortex`, which needs to be on your `fpath` before `compinit`:

```sh
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

??? note "Other forms"

    ```sh
    cortex completions zsh                 # print the script to stdout
    cortex completions bash --dir ./out    # write the correctly-named file
    cortex completions install --shell fish
    ```

    The bare-shell form is the stable interface for packagers.

## 5. Check it works

```sh
cortex device version
```

You should see your unit's firmware and serial number. The values below are fictional stand-ins; only the output shape is representative:

```text
device_type                QC
custom_name                Neural DSP Quad Cortex
serial_number              QA00AB123
coros_version              4.0.1
app_firmware               d14e
bootloader_firmware        d119
```

If instead you get `device not found`, work through: is it plugged in and powered on, is Cortex Control quit, did you replug after adding the udev rule.

Next: the [walkthrough](walkthrough.md).

To use the device from Claude Code or another local MCP harness, continue to [Agent setup](agent-setup.md).
