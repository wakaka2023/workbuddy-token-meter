@echo off
setlocal
REM ============================================================
REM  build_release.bat - clean release build for workbuddy-token-meter
REM
REM  Packages token-proxy.exe + tauri installers (NSIS/MSI) with a
REM  SANITIZED placeholder config so no real API keys are shipped.
REM  The private config is restored afterwards.
REM
REM  Usage:  build_release.bat
REM  After build, upload the artifacts with the gh command printed below.
REM ============================================================
cd /d %~dp0
set RES=token-widget\src-tauri\resources
set EXE_NAME=token-widget_0.1.0_x64-setup.exe
set MSI_NAME=token-widget_0.1.0_x64_en-US.msi

if not exist "%RES%\config.json" (
  echo [ERR] %RES%\config.json not found - run from repo root.
  exit /b 1
)

echo [1/5] backing up private config...
copy /y "%RES%\config.json" "%RES%\config.json.private.bak" >nul

echo [2/5] swapping in sanitized example config...
copy /y token-proxy\config.example.json "%RES%\config.json" >nul

echo [3/5] building token-proxy.exe (PyInstaller)...
pushd token-proxy
C:\Python\conda\envs\token-proxy\python.exe -m PyInstaller --clean -y token-proxy.spec || goto :fail
copy /y dist\token-proxy.exe "..\%RES%\token-proxy.exe" >nul || goto :fail
popd

echo [4/5] building tauri bundle (npm run tauri build)...
pushd token-widget
call npm run tauri build || goto :fail
popd

echo [5/5] restoring private config...
move /y "%RES%\config.json.private.bak" "%RES%\config.json" >nul

echo.
echo Build OK. Artifacts:
echo   token-proxy\dist\token-proxy.exe
echo   token-widget\src-tauri\target\release\bundle\nsis\%EXE_NAME%
echo   token-widget\src-tauri\target\release\bundle\msi\%MSI_NAME%
echo.
echo Upload command:
echo   gh release upload v0.1.0 token-proxy\dist\token-proxy.exe token-widget\src-tauri\target\release\bundle\nsis\%EXE_NAME% token-widget\src-tauri\target\release\bundle\msi\%MSI_NAME%
exit /b 0

:fail
echo [ERR] build failed, restoring private config...
if exist "%RES%\config.json.private.bak" move /y "%RES%\config.json.private.bak" "%RES%\config.json" >nul
popd
exit /b 1
