"""Owned companion witness for browser-only preflights, never registration mutation.

A PID alone is not authority. The retained setup parent must continue reporting
its same retained Child, and the installation binding must still match. These
checks establish an exclusion from process absence, not companion readiness.
"""
import os
from pathlib import Path
import re
import subprocess

IMAGES = ("firefox.exe", "download-manager-native-host.exe", "download-manager-setup.exe",
          "download-manager-app.exe", "download-manager-companion.exe")


def process_inventory():
    """Read fixed image names/IDs only; no paths, command lines or process control."""
    executable = Path(os.environ["WINDIR"]) / "System32/tasklist.exe"
    result = {}
    for name in IMAGES:
        observed = subprocess.run([str(executable), "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
                                  capture_output=True, timeout=20, check=True)
        if observed.stderr:
            raise RuntimeError("browser peer inventory unavailable")
        result[name] = parse_inventory(name, observed.stdout)
    return result


def parse_inventory(name, output):
    if name not in IMAGES or len(output) > 64 * 1024:
        raise RuntimeError("browser peer inventory exceeds scope")
    output = output.lower().strip()
    if output == b"info: no tasks are running which match the specified criteria.":
        return set()
    # Unknown/localized non-CSV messages are refusal, not evidence of absence.
    pattern = rb'"' + re.escape(name.encode("ascii")) + rb'","([1-9][0-9]{0,9})","[^"\r\n]*","[0-9]+","[^"\r\n]*"'
    identifiers = set()
    lines = output.splitlines()
    if not lines:
        raise RuntimeError("browser peer inventory is empty")
    for line in lines:
        row = re.fullmatch(pattern, line)
        if row is None or int(row[1]) > 2**32-1 or int(row[1]) in identifiers:
            raise RuntimeError("browser peer inventory is ambiguous")
        identifiers.add(int(row[1]))
    return identifiers


class BrowserPeer:
    def __init__(self, owner, binding, verify_binding, inventory=process_inventory):
        if binding is None or not callable(verify_binding):
            raise RuntimeError("verified installation binding required")
        self.owner, self.binding = owner, binding
        self.verify_binding, self.inventory = verify_binding, inventory
        self.parent = owner.process  # Exact retained Popen, not a discovered parent.
        state, child = owner._observe()
        if state != "complete" or child is None or owner.joined or owner.close_requested:
            raise RuntimeError("retained companion ownership unavailable")
        self.child = child
        self._witness()

    def _witness(self):
        if (self.owner.process is not self.parent or self.parent.poll() is not None
                or self.owner.joined or self.owner.close_requested):
            raise RuntimeError("retained setup witness unavailable")
        state, child = self.owner._observe()
        if state != "complete" or child not in (None, self.child):
            raise RuntimeError("retained companion witness changed")
        if self.verify_binding() != self.binding:
            raise RuntimeError("installed companion binding changed")
        # SetupOwner refuses disappearance/replacement of a previously observed
        # child; None now means that parent's explicit retained-child join receipt.
        return child

    def require_browser_closed(self):
        before = self._witness()
        observed = self.inventory()
        expected = {name: set() for name in IMAGES}
        expected["download-manager-setup.exe"] = {self.parent.pid}
        expected["download-manager-native-host.exe"] = set() if before is None else {before}
        if observed != expected or self._witness() != before:
            raise RuntimeError("Firefox/unowned helper remains or ownership changed")
