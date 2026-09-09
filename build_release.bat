@echo off
setlocal
REM ============================================================
REM  build_release.bat - clean release build for workbuddy-token-meter
REM
REM  Builds the tauri installers (NSIS/MSI) with a SANITIZED
REM  placeholder config so no real API keys are shipped.
REM  The private config is restored afterwards.
REM
REM  Note: bump VER below on every release, keep in sync with
REM  tauri.conf.json / Cargo.toml / package.json.
REM
REM  Usage:  build_release.bat
REM  After build, upload the artifacts with the gh command printed below.
REM ============================================================
cd /d %~dp0
set RES=token-widget\src-tauri\resources
set VER=0.3.0
set EXE_NAME=token-widget_%VER%_x64-setup.exe
set MSI_NAME=token-widget_%VER%_x64_en-US.msi

if not exist "%RES%\config.json" (
  echo [ERR] %RES%\config.json not found - run from repo root.
  exit /b 1
)
if not exist "%RES%\config.example.json" (
  echo [ERR] %RES%\config.example.json not found - need sanitized placeholder.
  exit /b 1
)

echo [1/4] backing up private config...
copy /y "%RES%\config.json" "%RES%\config.json.private.bak" >nul

echo [2/4] swapping in sanitized example config...
copy /y "%RES%\config.example.json" "%RES%\config.json" >nul

echo [3/4] building tauri bundle (npm run tauri build)...
pushd token-widget
call npm run tauri build || goto :fail
popd

echo [4/4] restoring private config...
if exist "%RES%\config.json.private.bak" move /y "%RES%\config.json.private.bak" "%RES%\config.json" >nul

echo.
echo Build OK. Artifacts:
echo   token-widget\src-tauri\target\release\bundle\nsis\%EXE_NAME%
echo   token-widget\src-tauri\target\release\bundle\msi\%MSI_NAME%
echo.
echo Upload command:
echo   gh release upload v%VER% token-widget\src-tauri\target\release\bundle\nsis\%EXE_NAME% token-widget\src-tauri\target\release\bundle\msi\%MSI_NAME%
exit /b 0

:fail
echo [ERR] build failed, restoring private config...
if exist "%RES%\config.json.private.bak" move /y "%RES%\config.json.private.bak" "%RES%\config.json" >nul
popd
exit /b 1