# Install

## Released Linux Install

- **Linux x86_64 only.** The leaf crate is portable, but released CLI/MCP host binaries currently support only Linux x86_64. Windows named pipes and native Windows/macOS lifecycle and hardware paths remain unimplemented or unverified.
- A **Quad Cortex or Nano Cortex**, connected by USB and powered on. Quad provides the full grid/session surface; Nano provides typed state plus non-persistent amp, bypass and raw FX parameter operations, while Gate reduction remains hardware-provisional.

```sh
curl -LsSf https://pacharanero.github.io/cortex/install.sh | sh
```

The script requires `curl` or `wget`, a SHA-256 tool, `tar`, `xz` (usually the `xz-utils` or `xz` package), and the coreutils `install` command. It downloads the latest GitHub Release archive, verifies its entry in the release's `SHA256SUMS`, then installs both `cortex` and `cortex-mcp` to `~/.local/bin` by default. It does not require Rust, a compiler, `protoc`, or development headers.

Set `CORTEX_VERSION=v0.1.0` to install a specific release, or `CORTEX_INSTALL_DIR=/some/bin` to choose a destination. Re-running it replaces both binaries and refreshes shell completions when the shell can be detected.

## Developer Build Requirements

Building from source instead requires Rust (stable), a C build toolchain, `pkg-config`, libudev development files, and `protoc`.

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

## 1. Grant Access To The Device

This is the step people get stuck on, and the symptom is confusing: the tool builds and installs perfectly, then reports that it cannot find a device that is plainly plugged in.

Both Cortex devices expose HID interface 5, and `/dev/hidraw*` nodes are root-only by default. Install the repository's rule with explicit entries for Quad `152a:880a` and Nano `152a:88e7`:

After the released installer, run the explicit setup step:

```sh
cortex setup --install-udev
```

It asks for `sudo` only to install `/etc/udev/rules.d/70-neural-dsp-cortex.rules`, reloads udev rules, and triggers hidraw. It never opens or changes the device. A source checkout can use the equivalent commands:

```sh
sudo install -m 0644 70-neural-dsp-cortex.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
sudo udevadm trigger --action=add --subsystem-match=hidraw
```

Then **unplug and replug the unit**.

`TAG+="uaccess"` grants access to whoever is logged in at the seat, via systemd-logind. That is preferable to adding yourself to a group: the permission follows the login session rather than being permanent, and there is no group to create.

??? question "How do I check it worked?"

    Find the node and look for the `+` that indicates an ACL:

    ```sh
    lsusb -d 152a:880a # Quad Cortex
    lsusb -d 152a:88e7 # Nano Cortex
    ls -l /dev/hidraw*
    ```

    You want a line like `crw-rw----+ 1 root root ... /dev/hidraw7`. Without the rule it reads `crw------- 1 root root`.

## 2. Quit Cortex Control

The protocol requires one effective HID owner, but the OS/device does not enforce it safely: a second process may open successfully and silently break the first owner's next request. If Neural DSP's Cortex Control is running, including in a VM with USB passthrough, quit it first.

The same applies in reverse: while `cortex` holds a session, Cortex Control will not connect.

## 3. Build From Source

```sh
git clone https://github.com/pacharanero/cortex
cd cortex
s/install
```

`s/install` builds and installs both `cortex` and `cortex-mcp` from the checkout. It exists because the obvious command does not work here: this repo is a Cargo workspace whose root manifest has no `[package]`, so `cargo install --path .` fails.

Maintainers can reproduce the non-publishing archive check with `s/release-preview`. It requires the exact cargo-dist version declared in the workspace manifest and never creates a tag, GitHub Release, or package publication.

It also checks that the canonical udev rule contains both products and prints the fix if it is missing or an older Quad-only rule is installed.

??? note "Options"

    ```sh
    s/install --force     # reinstall over an existing copy
    s/install --debug     # faster build, slower runtime
    s/install --root ~/.local
    s/install --cli-only  # omit cortex-mcp
    ```

    Any other flags are passed through to `cargo install`.

    `--mcp` remains accepted as a compatibility alias, but MCP is installed by default.

## 4. Diagnose And Configure

```sh
cortex setup
```

This read-only diagnostic reports architecture support, USB presence, whether the canonical udev rule is current, daemon health, and whether the paired MCP binary is installed. It deliberately does not open a HID handle, so it cannot disrupt Cortex Control or the held daemon.

To register the local stdio server with Claude Code, choose the explicit configuration action:

```sh
cortex setup --claude-code
```

This uses the absolute path of the sibling `cortex-mcp` binary and changes only Claude Code's user-scoped MCP configuration. Other harnesses should use the configuration in [Agent setup](agent-setup.md).

## 5. Shell Completions

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

## 6. Check It Works

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
