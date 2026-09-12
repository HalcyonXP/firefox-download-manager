"""Native controls for one retained setup and its retained companion; no physical input."""
import ctypes
from ctypes import wintypes as wt
import os
from pathlib import Path


class Guid(ctypes.Structure):
    _fields_ = [("a", wt.DWORD), ("b", wt.WORD), ("c", wt.WORD), ("d", wt.BYTE * 8)]


class Icon(ctypes.Structure):
    _fields_ = [("size", wt.DWORD), ("hwnd", wt.HWND), ("id", wt.UINT), ("guid", Guid)]


class Controls:
    def __init__(self):
        system = Path(os.environ["WINDIR"]) / "System32"
        self.user = ctypes.WinDLL(str(system / "user32.dll"), use_last_error=True)
        self.shell = ctypes.WinDLL(str(system / "shell32.dll"), use_last_error=True)
        self.callback = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
        declarations = {
            "EnumWindows": ([self.callback, wt.LPARAM], wt.BOOL),
            "GetWindowThreadProcessId": ([wt.HWND, ctypes.POINTER(wt.DWORD)], wt.DWORD),
            "GetClassNameW": ([wt.HWND, wt.LPWSTR, ctypes.c_int], ctypes.c_int),
            "GetDlgItem": ([wt.HWND, ctypes.c_int], wt.HWND),
            "GetWindowTextW": ([wt.HWND, wt.LPWSTR, ctypes.c_int], ctypes.c_int),
            "IsWindowVisible": ([wt.HWND], wt.BOOL),
            "SendMessageTimeoutW": ([wt.HWND, wt.UINT, wt.WPARAM, wt.LPARAM, wt.UINT, wt.UINT,
                                     ctypes.POINTER(ctypes.c_size_t)], wt.LPARAM),
        }
        for name, (arguments, result) in declarations.items():
            function = getattr(self.user, name)
            function.argtypes, function.restype = arguments, result
        self.shell.Shell_NotifyIconGetRect.argtypes = [ctypes.POINTER(Icon), ctypes.POINTER(wt.RECT)]
        self.shell.Shell_NotifyIconGetRect.restype = ctypes.c_long

    def pid(self, window):
        identity = wt.DWORD()
        self.user.GetWindowThreadProcessId(window, ctypes.byref(identity))
        return identity.value

    def find(self, process_id, kind):
        found = []
        @self.callback
        def visit(window, _):
            if self.pid(window) == process_id:
                name = ctypes.create_unicode_buffer(128)
                self.user.GetClassNameW(window, name, len(name))
                if name.value == kind:
                    found.append(window)
            return True
        if not self.user.EnumWindows(visit, 0):
            raise RuntimeError("owned window enumeration failed")
        return found[0] if len(found) == 1 else None

    def text(self, window, number, expected):
        control = self.user.GetDlgItem(window, number)
        if not control or self.pid(control) != expected:
            raise RuntimeError("owned control observation refused")
        value = ctypes.create_unicode_buffer(1024)
        if self.user.GetWindowTextW(control, value, len(value)) >= len(value)-1:
            raise RuntimeError("oversized owned control observation")
        return value.value

    def click(self, window, number, expected):
        control = self.user.GetDlgItem(window, number)
        if not control or self.pid(control) != expected:
            raise RuntimeError("owned control delivery refused")
        result = ctypes.c_size_t()
        if not self.user.SendMessageTimeoutW(control, 0xF5, 0, 0, 2, 2000, ctypes.byref(result)):
            raise RuntimeError("owned control delivery unconfirmed")

    def visible(self, window):
        return bool(self.user.IsWindowVisible(window))

    def tray(self, window):
        icon = Icon()
        icon.size, icon.hwnd, icon.id = ctypes.sizeof(icon), window, 1
        rectangle = wt.RECT()
        return self.shell.Shell_NotifyIconGetRect(ctypes.byref(icon), ctypes.byref(rectangle)) == 0
