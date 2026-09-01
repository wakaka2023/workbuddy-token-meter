@echo off
REM 启动 token 统计代理（Windows conda 环境 token-proxy）
REM main.py 已内置上游代理 127.0.0.1:7897，无需外部环境变量
"C:\Python\conda\envs\token-proxy\python.exe" "C:\Users\35317\WorkBuddy\token-proxy\main.py"
