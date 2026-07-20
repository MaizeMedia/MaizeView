"""Capture a MaizeView player HWND including child mpv surface."""
from __future__ import annotations

import array
import ctypes
import os
import struct
import subprocess
from ctypes import wintypes

user32 = ctypes.windll.user32
gdi32 = ctypes.windll.gdi32

EnumWindowsProc = ctypes.WINFUNCTYPE(ctypes.c_bool, wintypes.HWND, wintypes.LPARAM)

out = subprocess.check_output(
    ["powershell", "-NoProfile", "-Command", "(Get-Process maizeview -ErrorAction SilentlyContinue).Id"],
    text=True,
).strip().split()
pids = {int(x) for x in out if x.isdigit()}
print("pids", pids)
if not pids:
    raise SystemExit("no maizeview process")

targets: list[tuple[int, str]] = []


def cb(hwnd, _lparam):
    if not user32.IsWindowVisible(hwnd):
        return True
    pid = wintypes.DWORD()
    user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
    if pid.value not in pids:
        return True
    length = user32.GetWindowTextLengthW(hwnd)
    buf = ctypes.create_unicode_buffer(length + 1)
    user32.GetWindowTextW(hwnd, buf, length + 1)
    title = buf.value
    if title:
        targets.append((int(hwnd), title))
    return True


user32.EnumWindows(EnumWindowsProc(cb), 0)
for h, t in targets:
    print(hex(h), t)

player = None
for h, t in targets:
    if t == "MaizeView":
        continue
    if "Player" in t or "YOUMIX" in t or "debug" in t.lower() or t.startswith("MaizeView"):
        player = (h, t)
        break
if not player:
    raise SystemExit("no player window")

hwnd, title = player
print("capturing", hex(hwnd), title)

rect = wintypes.RECT()
user32.GetWindowRect(hwnd, ctypes.byref(rect))
w = rect.right - rect.left
h = rect.bottom - rect.top
print("size", w, h)

hdc = user32.GetDC(hwnd)
mem = gdi32.CreateCompatibleDC(hdc)
bmp = gdi32.CreateCompatibleBitmap(hdc, w, h)
old = gdi32.SelectObject(mem, bmp)
ok = user32.PrintWindow(hwnd, mem, 2)  # PW_RENDERFULLCONTENT
print("PrintWindow", ok)
if not ok:
    gdi32.BitBlt(mem, 0, 0, w, h, hdc, 0, 0, 0x00CC0020)


class BITMAPINFOHEADER(ctypes.Structure):
    _fields_ = [
        ("biSize", wintypes.DWORD),
        ("biWidth", ctypes.c_long),
        ("biHeight", ctypes.c_long),
        ("biPlanes", wintypes.WORD),
        ("biBitCount", wintypes.WORD),
        ("biCompression", wintypes.DWORD),
        ("biSizeImage", wintypes.DWORD),
        ("biXPelsPerMeter", ctypes.c_long),
        ("biYPelsPerMeter", ctypes.c_long),
        ("biClrUsed", wintypes.DWORD),
        ("biClrImportant", wintypes.DWORD),
    ]


bih = BITMAPINFOHEADER()
bih.biSize = ctypes.sizeof(BITMAPINFOHEADER)
bih.biWidth = w
bih.biHeight = -h
bih.biPlanes = 1
bih.biBitCount = 32
bih.biCompression = 0
buf = (ctypes.c_char * (w * h * 4))()
gdi32.GetDIBits(mem, bmp, 0, h, buf, ctypes.byref(bih), 0)

a = array.array("B", bytes(buf))
cx0, cy0 = max(0, w // 2 - 100), max(0, h // 2 - 100)
vals = []
for y in range(cy0, min(h, cy0 + 200)):
    for x in range(cx0, min(w, cx0 + 200)):
        i = (y * w + x) * 4
        vals.append((a[i] + a[i + 1] + a[i + 2]) / 3)
avg = sum(vals) / max(1, len(vals))
nonzero = sum(1 for v in vals if v > 5)
print(f"center avg brightness={avg:.1f} nonzero_px={nonzero}/{len(vals)}")

path = os.path.join("docs", "e2e-reports", "player-hwnd.bmp")
os.makedirs(os.path.dirname(path), exist_ok=True)
row = ((w * 3 + 3) // 4) * 4
pixels = bytearray()
for y in range(h - 1, -1, -1):
    rowb = bytearray()
    for x in range(w):
        i = (y * w + x) * 4
        rowb += bytes([a[i], a[i + 1], a[i + 2]])
    rowb += b"\x00" * (row - w * 3)
    pixels += rowb
with open(path, "wb") as f:
    fsize = 14 + 40 + len(pixels)
    f.write(b"BM" + struct.pack("<IHHI", fsize, 0, 0, 54))
    f.write(struct.pack("<IiiHHIIiiII", 40, w, h, 1, 24, 0, len(pixels), 0, 0, 0, 0))
    f.write(pixels)
print("wrote", path, "bytes", os.path.getsize(path))

gdi32.SelectObject(mem, old)
gdi32.DeleteObject(bmp)
gdi32.DeleteDC(mem)
user32.ReleaseDC(hwnd, hdc)
