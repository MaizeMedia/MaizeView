@echo off
REM Helper: run cargo with the MSVC toolchain env active and Git's /usr/bin
REM removed from PATH so MSVC's link.exe isn't shadowed. Called from Git Bash.
REM Usage: cargo-msvc.bat <cargo args...>

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul 2>nul

REM Strip the Git coreutils usr/bin dir so GNU `link`/`find` don't shadow MSVC.
set "PATH=%PATH:C:\Program Files\Git\usr\bin;=%"

REM Rustup/cargo live here on a default install.
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

cargo %*
