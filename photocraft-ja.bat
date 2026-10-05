@echo off
rem Launch PhotoCraft with the Japanese UI.
rem Usage: double-click, or run from a terminal. An image path can be passed: photocraft-ja.bat image.psd
rem To switch back to English, set PHOTOCRAFT_LANG=en (or run cargo run directly).
setlocal
cd /d "%~dp0"
set PHOTOCRAFT_LANG=ja
if "%~1"=="" (
  cargo run --release -p photocraft
) else (
  cargo run --release -p photocraft -- %*
)
if errorlevel 1 pause
endlocal
