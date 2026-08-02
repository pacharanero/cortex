# `s/`

Small scripts naming the repeated processes of working on this repository. Each is a thin wrapper round a real tool; the script is the canonical way to invoke that process, so a fresh clone finds the right entry point by listing one directory.

Run them from anywhere in the repo.

## `s/test`

Run the test suite across the workspace.

- `s/test` - everything

## `s/lint`

The full local gate, mirroring CI: `cargo fmt --check`, clippy with `-D warnings` on both feature paths, the tests, and `reuse lint`. A green run here means a green run in CI.

- `s/lint`

## `s/install`

Install the `cortex` CLI from this checkout onto your `PATH`.

Needed because the obvious command does not work here: the workspace root has no `[package]`, so `cargo install --path .` fails. The binary lives in `crates/cortex-cli`.

- `s/install` - install `cortex`
- `s/install --force` - reinstall over an existing copy
- `s/install --debug` - faster build, slower runtime
- `s/install --mcp` - also install `cortex-mcp` (currently a scaffold)

Also preflights the udev rule and prints the fix if it is missing.

## `s/lint-no-device-data`

Fail if anything shaped like real device data - a serial number, MAC address, or firmware checksum - has crept into a tracked file. Run by `s/lint` and by CI.

Documentation examples are captured from a physical unit, so this is easy to do by accident and impossible to undo once published. Works by allow-list rather than deny-list, so the values being kept out never have to be committed.

- `s/lint-no-device-data`

## `s/progress`

Where the project is up to, counted from `spec/roadmap.md` - the single progress record. Counted rather than written, so the summary cannot drift from the thing it summarises.

- `s/progress` - per-section totals
- `s/progress --wip` - what is in progress
- `s/progress --next` - what is planned

## `s/docs`

Serve the documentation site locally with hot reload, on the first free port in 8000-8030, opening a browser.

One-off setup:

```sh
python3 -m venv .venv
./.venv/bin/pip install -r requirements.txt
```

- `s/docs`
- `s/docs --dev-addr localhost:9000` - override the auto-picked port

## `s/docs-cli-reference`

Regenerate `docs/cli-reference.md` from the CLI's own `--help`, so the reference cannot drift from the real command surface. Run after adding or changing a command, and commit the result.

- `s/docs-cli-reference`
