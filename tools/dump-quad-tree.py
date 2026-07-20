"""Dump child-window trees of MaizeView player/quad windows with full styles.

Usage: python tools/dump-quad-tree.py [title-substring]   (default "4Play")
"""
from __future__ import annotations

import ctypes
import subprocess
import sys
from ctypes import wintypes

user32 = ctypes.windll.user32
EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)

GWL_STYLE = -16
GWL_EXSTYLE = -20
WS_CHILD = 0x40000000
WS_VISIBLE = 0x10000000
WS_EX_TRANSPARENT = 0x00000020
WS_EX_LAYERED = 0x00080000
WS_EX_NOREDIRECTIONBITMAP = 0x00200000


def text_of(hwnd: int) -> str:
    n = user32.GetWindowTextLengthW(hwnd)
    buf = ctypes.create_unicode_buffer(n + 1)
    user32.GetWindowTextW(hwnd, buf, n + 1)
    return buf.value


def class_of(hwnd: int) -> str:
    buf = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(hwnd, buf, 256)
    return buf.value


def maizeview_pids() -> set[int]:
    out = subprocess.check_output(
        ["powershell", "-NoProfile", "-Command", "(Get-Process maizeview -ErrorAction SilentlyContinue).Id"],
        text=True,
    ).strip().split()
    return {int(x) for x in out if x.isdigit()}


def find_tops(pids: set[int], needle: str) -> list[int]:
    found: list[int] = []

    def cb(hwnd, _lp):
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids and needle.lower() in text_of(hwnd).lower():
            found.append(int(hwnd))
        return True

    user32.EnumWindows(EnumWindowsProc(cb), 0)
    return found


def dump(hwnd: int, depth: int) -> None:
    rc = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rc))
    style = user32.GetWindowLongW(hwnd, GWL_STYLE) & 0xFFFFFFFF
    exstyle = user32.GetWindowLongW(hwnd, GWL_EXSTYLE) & 0xFFFFFFFF
    vis = "V" if style & WS_VISIBLE else "-"
    child = "C" if style & WS_CHILD else "-"
    extra = []
    if exstyle & WS_EX_LAYERED:
        extra.append("layered")
    if exstyle & WS_EX_TRANSPARENT:
        extra.append("ex-transparent")
    if exstyle & WS_EX_NOREDIRECTIONBITMAP:
        extra.append("no-redir")
    cls = class_of(hwnd)
    txt = text_of(hwnd).replace("\n", " ")[:30]
    print(
        f"{'  ' * depth}{hwnd:#010x} [{vis}{child}] "
        f"({rc.left},{rc.top})-({rc.right},{rc.bottom}) "
        f"style={style:#010x} ex={exstyle:#010x} {cls!r} {txt!r} {' '.join(extra)}"
    )

    def child_cb(chwnd, _lp):
        dump(int(chwnd), depth + 1)
        return True

    user32.EnumChildWindows(hwnd, EnumWindowsProc(child_cb), 0)


needle = sys.argv[1] if len(sys.argv) > 1 else "4Play"
pids = maizeview_pids()
if not pids:
    raise SystemExit("no maizeview process")
tops = find_tops(pids, needle)
if not tops:
    raise SystemExit(f"no window matching {needle!r}")
for top in tops:
    print(f"=== {top:#x} {text_of(top)!r}")
    dump(top, 0)
