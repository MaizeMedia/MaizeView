@echo off
REM Run an npm/npx command with Node on PATH. Usage: run-node.bat <cmd> [args...]
REM where <cmd> is npm or npx.
set "PATH=C:\Program Files\nodejs;%PATH%"
%*
