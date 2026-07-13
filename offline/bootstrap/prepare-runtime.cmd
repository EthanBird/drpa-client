@echo off
setlocal
set "ROOT=%~dp0"
for /d %%D in ("%ROOT%python\cpython-3.11.9-*") do set "PYTHON=%%D\python.exe"
if not defined PYTHON (
  echo [DRPA offline] bundled Python 3.11.9 is missing.
  exit /b 1
)
"%PYTHON%" "%ROOT%bootstrap_runtime.py"
exit /b %errorlevel%
