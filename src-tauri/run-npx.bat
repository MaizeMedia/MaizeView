@echo off
REM Run npx with Node on PATH. Usage: run-npx.bat <pkg> [args...]
set "PATH=C:\Program Files\nodejs;%PATH%"
npx %*
