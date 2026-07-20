"""Screen-grab a MaizeView window by title substring (what the user really sees).

Usage: python tools/screen-grab.py <title-substring> <out.bmp>
Uses BitBlt from the screen DC, so D3D/DWM-composited content is included.
"""
from __future__ import annotations

import ctypes
import os
import struct
import subprocess
import sys
from ctypes import wintypes

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32
EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)
SRCCOPY = 0x00CC0020


def text_of(hwnd: int) -> str:
    n = user32.GetWindowTextLengthW(hwnd)
    buf = ctypes.create_unicode_buffer(n + 1)
    user32.GetWindowTextW(hwnd, buf, n + 1)
    return buf.value


needle, out_path = sys.argv[1], sys.argv[2]
out = subprocess.check_output(
    ["powershell", "-NoProfile", "-Command", "(Get-Process maizeview -ErrorAction SilentlyContinue).Id"],
    text=True,
).strip().split()
pids = {int(x) for x in out if x.isdigit()}
found: list[int] = []


def cb(hwnd, _lp):
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    if pid.value in pids and needle.lower() in text_of(hwnd).lower() and user32.IsWindowVisible(hwnd):
        found.append(int(hwnd))
    return True


user32.EnumWindows(EnumWindowsProc(cb), 0)
if not found:
    raise SystemExit(f"no visible window matching {needle!r}")
hwnd = found[0]
print(f"grabbing {hwnd:#x} {text_of(hwnd)!r}")

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
os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
row = ((w * 3 + 3) // 4) * 4
px = bytearray()
for y in range(h - 1, -1, -1):
    for x in range(w):
        i = (y * w + x) * 4
        px += bytes([a[i], a[i + 1], a[i + 2]])
    px += b"\x00" * (row - w * 3)
with open(out_path, "wb") as f:
    f.write(b"BM" + struct.pack("<IHHI", 14 + 40 + len(px), 0, 0, 54))
    f.write(struct.pack("<IiiHHIIiiII", 40, w, h, 1, 24, 0, len(px), 0, 0, 0, 0))
    f.write(px)
print(f"wrote {out_path} ({w}x{h})")
gdi32.SelectObject(mem, old)
gdi32.DeleteObject(bmp)
gdi32.DeleteDC(mem)
user32.ReleaseDC(None, screen)
