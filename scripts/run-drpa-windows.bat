@echo off
setlocal enabledelayedexpansion

cd /d "%~dp0\.."
set "ROOT=%CD%"
set "VENV_PY=%ROOT%\.venv\Scripts\python.exe"
set "VENV_MODULE=drpa_client.app.main"
set "UV_DIR=%ROOT%\.tools\uv"
set "UV_EXE=%UV_DIR%\uv.exe"
set "UV_PYTHON_INSTALL_DIR=%ROOT%\.tools\python"

for /d %%D in ("%ROOT%\.tools\python\cpython-3.11.9-*") do set "FIXER=%%D\python.exe"
if not defined FIXER for /d %%D in ("%ROOT%\.tools\python\windows\cpython-3.11.9-*") do set "FIXER=%%D\python.exe"

if defined FIXER if exist "%FIXER%" (
  "%FIXER%" "%ROOT%\tools\fix_offline_runtime.py"
  if errorlevel 1 (
    echo.
    echo [DRPA] Failed to prepare offline runtime.
    echo See messages above for details.
    pause
    exit /b 1
  )
)

if exist "%VENV_PY%" (
  "%VENV_PY%" -c "import drpa_client" >nul 2>&1
  if not errorlevel 1 (
    echo [DRPA] Starting DRPA Client (offline mode)...
    "%VENV_PY%" -m %VENV_MODULE%
    set "EXIT_CODE=!errorlevel!"
    if !EXIT_CODE! neq 0 (
      echo.
      echo [DRPA] Launch failed. Running diagnostics...
      "%VENV_PY%" -c "import sys; print('python =', sys.executable); import drpa_client; print('drpa_client =', drpa_client.__file__); import PySide6; print('PySide6 =', PySide6.__file__)"
      pause
    )
    exit /b !EXIT_CODE!
  )
)

if not "%DRPA_ALLOW_ONLINE%"=="1" (
  echo [DRPA] Offline startup failed.
  echo.
  echo Required paths:
  echo   %ROOT%\.venv\
  echo   %ROOT%\.tools\python\
  echo.
  echo For online development, set DRPA_ALLOW_ONLINE=1 and retry.
  pause
  exit /b 1
)

if not exist "%UV_EXE%" (
  echo [DRPA] uv not found. Downloading standalone uv...
  powershell -NoProfile -ExecutionPolicy Bypass -Command ^
    "$ErrorActionPreference='Stop';" ^
    "$dir='%UV_DIR%';" ^
    "New-Item -ItemType Directory -Force -Path $dir | Out-Null;" ^
    "$zip=Join-Path $dir 'uv.zip';" ^
    "Invoke-WebRequest -Uri 'https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip' -OutFile $zip;" ^
    "Expand-Archive -Force -Path $zip -DestinationPath $dir;" ^
    "$uv=Get-ChildItem -Path $dir -Recurse -Filter uv.exe | Select-Object -First 1;" ^
    "if (-not $uv) { throw 'uv.exe not found after extraction' };" ^
    "Copy-Item -Force $uv.FullName (Join-Path $dir 'uv.exe');" ^
    "Remove-Item -Force $zip"
  if errorlevel 1 (
    echo [DRPA] Failed to download uv.
    pause
    exit /b 1
  )
)

echo [DRPA] Online mode: syncing runtime via uv...
"%UV_EXE%" sync --reinstall-package drpa-client
if errorlevel 1 (
  echo [DRPA] uv sync failed.
  pause
  exit /b 1
)

echo [DRPA] Starting DRPA Client...
"%UV_EXE%" run drpa-client
if errorlevel 1 (
  echo [DRPA] DRPA Client exited with error.
  pause
  exit /b 1
)

endlocal
