"""Bounded diagnostic source locations, never process/policy/cleanup authority.

No exception messages, arguments, frame locals, source text or raw paths are
serialized. The source revision must be recorded separately by the run owner.
"""
import os
from pathlib import Path

_ROOT = Path(__file__).resolve().parents[2]
_SOURCES = {
    os.path.normcase(str(_ROOT / relative)): label
    for relative, label in (
        ('scripts/qualification/browser_peer.py', 'browser-peer'),
        ('scripts/qualification/firefox.py', 'browser'),
        ('scripts/qualification/installed.py', 'installed'),
        ('scripts/qualification/parent_installed.py', 'parent-installed'),
        ('scripts/qualification/setup_owner.py', 'setup-owner'),
        ('scripts/qualification/installed_ui.py', 'setup-ui'),
        ('scripts/test-package-install.py', 'package-preflight'),
    )
}


def failure_location(error):
    """Copy at most eight recognized locations within a 32-frame observation.

    Unrecognized frames consume the observation budget but reveal no paths or
    line numbers. A location only describes where an exception propagated; it
    does not establish the underlying cause or authorize recovery operations.
    """
    if not isinstance(error, BaseException):
        raise TypeError('diagnostic exception required')
    trace = BaseException.__getattribute__(error, '__traceback__')
    locations = []
    for _ in range(32):
        if trace is None:
            break
        label = _SOURCES.get(os.path.normcase(trace.tb_frame.f_code.co_filename))
        if label is not None:
            line = trace.tb_lineno
            if type(line) is int and 0 < line <= 65535:
                locations.append({'source': label, 'line': line})
                locations = locations[-8:]
        trace = trace.tb_next
    return {'locations': locations, 'trace_truncated': trace is not None}
