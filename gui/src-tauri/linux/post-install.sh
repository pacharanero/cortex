#!/bin/sh
# SPDX-FileCopyrightText: 2026 Dr Marcus Baw
# SPDX-License-Identifier: AGPL-3.0-or-later

# Activate the packaged rule for devices that were connected during install.
# Minimal containers may have no running udev daemon, which must not make the
# package transaction fail.
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules >/dev/null 2>&1 || true
    udevadm trigger --action=add --subsystem-match=hidraw >/dev/null 2>&1 || true
fi

exit 0
