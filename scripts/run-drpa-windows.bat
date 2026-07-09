@echo off
setlocal enabledelayedexpansion

cd /d "%~dp0\.."

set "UV_DIR=%CD%\.tools\uv"
set "UV_EXE=%UV_DIR%\uv.exe"

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
    "Copy-Item -Force $uv.FullName (Join-Path $dir 'uv.exe');"
  if errorlevel 1 (
    echo [DRPA] Failed to download uv.
    pause
    exit /b 1
  )
)

echo [DRPA] Syncing runtime. Python is managed by uv via .python-version.
"%UV_EXE%" sync
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
