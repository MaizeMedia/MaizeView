"""Decisive experiment for the 4Play paint bug: reparent mpv to the top window.

Theory: in the working single player, mpv's render window is a DIRECT child of
the Tauri top window. In the quad spike it's a child of a Static pane (a
grandchild of the top window), and it doesn't paint. This script:

  1. Screen-grabs the quad window region (BEFORE — what the user really sees).
  2. SetParent(mpv_window, top_window) + reposition to the Q1 slot.
  3. Screen-grabs again (AFTER).

Screen grabs use BitBlt from the screen DC, so they capture exactly what DWM
composes (D3D content included) — unlike PrintWindow, which misses no-redir
layers. Images land in docs/e2e-reports/quad-reparent-{before,after}.bmp.
"""
from __future__ import annotations

import ctypes
import os
import struct
import subprocess
import time
from ctypes import wintypes

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
SRCCOPY = 0x00CC0020
SWP_NOZORDER = 0x0004
SWP_NOACTIVATE = 0x0010


def text_of(hwnd: int) -> str:
    n = user32.GetWindowTextLengthW(hwnd)
    buf = ctypes.create_unicode_buffer(n + 1)
    user32.GetWindowTextW(hwnd, buf, n + 1)
    return buf.value


def class_of(hwnd: int) -> str:
    buf = ctypes.create_unicode_buffer(256)
    user32.GetClassNameW(hwnd, buf, 256)
    return buf.value


def pids() -> set[int]:
    out = subprocess.check_output(
        ["powershell", "-NoProfile", "-Command", "(Get-Process maizeview -ErrorAction SilentlyContinue).Id"],
        text=True,
    ).strip().split()
    return {int(x) for x in out if x.isdigit()}


def find_quad():
    top = None

    def cb(hwnd, _lp):
        nonlocal top
        pid = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in PIDS and "4Play" in text_of(hwnd):
            top = int(hwnd)
            return False
        return True

    PIDS = pids()
    user32.EnumWindows(EnumWindowsProc(cb), 0)
    if top is None:
        raise SystemExit("no 4Play window")
    mpv = None
    pane = None

    def child_cb(hwnd, _lp):
        nonlocal mpv, pane
        cls = class_of(hwnd)
        if cls == "mpv" and mpv is None:
            mpv = int(hwnd)
        if cls == "Static" and pane is None:
            pane = int(hwnd)
        return True

    user32.EnumChildWindows(top, EnumWindowsProc(child_cb), 0)
    if mpv is None or pane is None:
        raise SystemExit(f"mpv={mpv} pane={pane} not found")
    return top, mpv, pane


def grab(hwnd: int, path: str) -> None:
    rc = wintypes.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rc))
    w, h = rc.right - rc.left, rc.bottom - rc.top
    screen = user32.GetDC(None)
    mem = gdi32.CreateCompatibleDC(screen)
    bmp = gdi32.CreateCompatibleBitmap(screen, w, h)
    old = gdi32.SelectObject(mem, bmp)
    gdi32.BitBlt(mem, 0, 0, w, h, screen, rc.left, rc.top, SRCCOPY)

    class BIH(ctypes.Structure):
        _fields_ = [(f, t) for f, t in [
            ("biSize", wintypes.DWORD), ("biWidth", ctypes.c_long), ("biHeight", ctypes.c_long),
            ("biPlanes", wintypes.WORD), ("biBitCount", wintypes.WORD), ("biCompression", wintypes.DWORD),
            ("biSizeImage", wintypes.DWORD), ("biXP", ctypes.c_long), ("biYP", ctypes.c_long),
            ("biClrUsed", wintypes.DWORD), ("biClrImportant", wintypes.DWORD)]]

    bih = BIH(ctypes.sizeof(BIH), w, -h, 1, 32, 0, 0, 0, 0, 0, 0)
    buf = (ctypes.c_char * (w * h * 4))()
    gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bih), 0)
    a = bytes(buf)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    row = ((w * 3 + 3) // 4) * 4
    px = bytearray()
    for y in range(h - 1, -1, -1):
        for x in range(w):
            i = (y * w + x) * 4
            px += bytes([a[i], a[i + 1], a[i + 2]])
        px += b"\x00" * (row - w * 3)
    with open(path, "wb") as f:
        f.write(b"BM" + struct.pack("<IHHI", 14 + 40 + len(px), 0, 0, 54))
        f.write(struct.pack("<IiiHHIIiiII", 40, w, h, 1, 24, 0, len(px), 0, 0, 0, 0))
        f.write(px)
    # quick brightness sample over Q1 (where video should be)
    vals = []
    for y in range(40, min(h, 360), 7):
        for x in range(20, min(w, 620), 7):
            i = (y * w + x) * 4
            vals.append((a[i] + a[i + 1] + a[i + 2]) / 3)
    avg = sum(vals) / max(1, len(vals))
    print(f"{path}: {w}x{h} Q1 avg brightness={avg:.1f}")
    gdi32.SelectObject(mem, old)
    gdi32.DeleteObject(bmp)
    gdi32.DeleteDC(mem)
    user32.ReleaseDC(None, screen)


top, mpv, pane = find_quad()
print(f"top={top:#x} mpv={mpv:#x} pane0={pane:#x}")
grab(top, "docs/e2e-reports/quad-reparent-before.bmp")

prev = user32.SetParent(mpv, top)
print(f"SetParent(mpv, top) -> previous parent {prev:#x} (pane0 was {pane:#x})")
# mpv now uses top's client coords: Q1 = (0,0)-(640,360)
user32.SetWindowPos(mpv, None, 0, 0, 640, 360, SWP_NOZORDER | SWP_NOACTIVATE)
time.sleep(1.5)
grab(top, "docs/e2e-reports/quad-reparent-after.bmp")
print("done — compare the two BMPs")
