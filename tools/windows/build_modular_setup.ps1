param(
    [Parameter(Mandatory = $true)]
    [string]$Stage,
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [Parameter(Mandatory = $true)]
    [string]$Output,
    [string]$ComponentsOutput = "",
    [string]$Python314Archive = "",
    [switch]$SkipPython314Minimal,
    [switch]$CoreOnly,
    [string]$Makensis = "C:\Program Files (x86)\NSIS\makensis.exe",
    [string]$Icon = "apps\desktop\src-tauri\icons\icon.ico"
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$stageRoot = (Resolve-Path $Stage).Path
$outputPath = [IO.Path]::GetFullPath($Output)
$outputDirectory = Split-Path -Parent $outputPath
if ([string]::IsNullOrWhiteSpace($ComponentsOutput)) {
    $componentOutputPath = Join-Path $outputDirectory "components"
}
else {
    $componentOutputPath = [IO.Path]::GetFullPath($ComponentsOutput)
}
$iconPath = [IO.Path]::GetFullPath((Join-Path $repoRoot $Icon))
$coreStage = Join-Path $outputDirectory (".drpa-core-stage-" + [Guid]::NewGuid().ToString("N"))

if (-not (Test-Path -LiteralPath (Join-Path $stageRoot "DRPA Next.exe") -PathType Leaf)) {
    throw "Stage is missing DRPA Next.exe: $stageRoot"
}
if (-not (Test-Path -LiteralPath (Join-Path $stageRoot "runtime\manifest.json") -PathType Leaf)) {
    throw "Stage is missing the sealed runtime manifest: $stageRoot"
}
if (-not (Test-Path -LiteralPath (Join-Path $stageRoot "webview2\msedgewebview2.exe") -PathType Leaf)) {
    throw "Stage is missing the fixed WebView2 runtime: $stageRoot"
}
if (-not (Test-Path -LiteralPath $Makensis -PathType Leaf)) {
    throw "NSIS compiler is missing: $Makensis"
}

Push-Location $repoRoot
try {
    New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
    New-Item -ItemType Directory -Force -Path $componentOutputPath | Out-Null
    New-Item -ItemType Directory -Force -Path $coreStage | Out-Null

    if ($CoreOnly) {
        cargo build --release -p drpa-cli -p drpa-launcher -p drpa-component-installer
        if ($LASTEXITCODE -ne 0) { throw "DRPA Core, Launcher or component installer build failed" }
    }
    else {
        npm run build --workspace @drpa/desktop
        if ($LASTEXITCODE -ne 0) { throw "DRPA desktop frontend build failed" }
        cargo build --release -p drpa-desktop -p drpa-cli -p drpa-launcher -p drpa-component-installer --features drpa-desktop/custom-protocol
        if ($LASTEXITCODE -ne 0) { throw "DRPA desktop, Core, Launcher or component installer build failed" }

        Copy-Item -LiteralPath (Join-Path $repoRoot "target\release\drpa-desktop.exe") -Destination (Join-Path $stageRoot "DRPA Next.exe") -Force
        python tools/offline/sync_runtime_adapter.py --runtime-root (Join-Path $stageRoot "runtime")
        if ($LASTEXITCODE -ne 0) { throw "DRPA Python adapter sync failed" }

        python tools/windows/stage_component_packs.py --stage $stageRoot --version $Version --output $componentOutputPath --components-only
        if ($LASTEXITCODE -ne 0) { throw "Component split failed" }
        if (-not $SkipPython314Minimal) {
            $minimalOutput = Join-Path $componentOutputPath "org.drpa.python-runtime.py314-minimal.drpac"
            $minimalCache = Join-Path $repoRoot "artifacts\cache\python314"
            $minimalArgs = @(
                "tools/windows/build_python314_minimal.py",
                "--output", $minimalOutput,
                "--component-version", $Version,
                "--cache", $minimalCache
            )
            if (-not [string]::IsNullOrWhiteSpace($Python314Archive)) {
                $minimalArgs += @("--source-archive", [IO.Path]::GetFullPath($Python314Archive))
            }
            python @minimalArgs
            if ($LASTEXITCODE -ne 0) { throw "Python 3.14 Minimal component build failed" }
        }
    }

    Copy-Item -LiteralPath (Join-Path $repoRoot "target\release\drpa.exe") -Destination (Join-Path $coreStage "drpa.exe") -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot "target\release\drpa-launcher.exe") -Destination (Join-Path $coreStage "DRPA Next.exe") -Force
    Copy-Item -LiteralPath (Join-Path $repoRoot "target\release\drpa-component-installer.exe") -Destination (Join-Path $coreStage "DRPA Component Installer.exe") -Force
    $coreFiles = @("DRPA Next.exe", "drpa.exe", "DRPA Component Installer.exe", "core-files.json")
    $readme = Join-Path $stageRoot "README.md"
    if (Test-Path -LiteralPath $readme -PathType Leaf) {
        Copy-Item -LiteralPath $readme -Destination (Join-Path $coreStage "README.md") -Force
        $coreFiles += "README.md"
    }
    $coreManifest = @{ schema = 1; files = $coreFiles } | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText((Join-Path $coreStage "core-files.json"), $coreManifest + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))

    & $Makensis "/INPUTCHARSET" "UTF8" "/DPAYLOAD_DIR=$coreStage" "/DOUTPUT_FILE=$outputPath" "/DICON_FILE=$iconPath" "installer\windows\drpa-next.nsi"
    if ($LASTEXITCODE -ne 0) { throw "NSIS setup build failed" }

    $registryCalls = Select-String -Path "installer\windows\drpa-next.nsi" -Pattern "WriteReg|DeleteReg|ReadReg|InstallDirRegKey" -CaseSensitive:$false
    if ($registryCalls) { throw "Installer contains a forbidden registry instruction" }
    Write-Host "Built registry-free lightweight Core Setup: $outputPath"
    Write-Host "Built independent component packs: $componentOutputPath"
}
finally {
    Pop-Location
    if (Test-Path -LiteralPath $coreStage -PathType Container) {
        $resolvedCoreStage = (Resolve-Path -LiteralPath $coreStage).Path
        $resolvedOutputDirectory = [IO.Path]::GetFullPath($outputDirectory).TrimEnd([IO.Path]::DirectorySeparatorChar)
        $expectedPrefix = $resolvedOutputDirectory + [IO.Path]::DirectorySeparatorChar
        if (-not $resolvedCoreStage.StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to remove core stage outside output directory: $resolvedCoreStage"
        }
        Remove-Item -LiteralPath $resolvedCoreStage -Recurse -Force
    }
}
