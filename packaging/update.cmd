@echo off
rem Copies the latest build (target\release) over the installed Crest. Just double-click.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0update.ps1"
if errorlevel 1 pause
