# Security Policy

## Reporting A Vulnerability

Do not report suspected vulnerabilities through public issues.

Report them privately through [GitHub's private vulnerability reporting](https://github.com/pacharanero/cortex/security/advisories/new) for this repository.

Include a description of the issue and impact, affected versions or commit hashes, safe reproduction steps, and any suggested mitigation. Redact credentials and any real device data (serial numbers, MAC addresses, firmware checksums, preset or Neural Capture names).

We aim to acknowledge reports within 5 working days and provide a substantive response within 30 days.

## Scope

This policy covers the `cortex-rs` crate, the `cortex` CLI, `cortex-mcp`, `cortex-host`, and the Tauri GUI in this repository. Report upstream dependency vulnerabilities to the relevant maintainer and notify us privately if they affect this project.

USB HID device communication is inherently local: the primary risk surface is a malicious or malformed device response, a compromised host process with local socket access to `cortex session`, or an MCP client invoking mutating tools without appropriate confirmation. Reports about any of these are especially welcome.

## Disclosure

We use coordinated disclosure. After a fix is available, we will publish an appropriate advisory or release note and credit the reporter if they agree.
