@echo off
title token-proxy dev (source)
cd /d %~dp0
set TOKEN_PROXY_DATA_DIR=C:\Users\35317\AppData\Roaming\com.tauri-app.token-widget\token-proxy
echo [token-proxy dev] starting source version...
echo Ctrl+C to stop. Close this window to stop the proxy.
echo.
C:\Python\conda\envs\token-proxy\python.exe main.py
pause
