#!/usr/bin/env pwsh
<#
.SYNOPSIS
    构建 StudyAgent 的 Windows 安装程序（Inno Setup）。

.DESCRIPTION
    流程：
      1. 构建 Tauri 应用（tauri.conf.json 中 bundle.active=false，只产出 exe）
      2. 把待打包文件复制到 installer\staging
      3. 调用 ISCC.exe 编译 installer\StudyAgent.iss
    产物：
      desktop\src-tauri\target\release\bundle\inno\StudyAgent_<version>_x64-setup.exe

.PARAMETER SkipBuild
    跳过 Tauri 构建，仅用已有的 target\release\studyagent-desktop.exe 重新打包。

.EXAMPLE
    .\build.ps1
    .\build.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$InstallerDir = $PSScriptRoot
$DesktopDir = Split-Path -Parent $InstallerDir
$TauriDir = Join-Path $DesktopDir 'src-tauri'
$ReleaseDir = Join-Path $TauriDir 'target\release'
$StagingDir = Join-Path $InstallerDir 'staging'
$OutputDir = Join-Path $ReleaseDir 'bundle\inno'
$IssFile = Join-Path $InstallerDir 'StudyAgent.iss'
$ExeName = 'studyagent-desktop.exe'

function Get-IsccPath {
    $candidates = @(
        (Get-Command 'ISCC.exe' -ErrorAction SilentlyContinue).Source
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe')
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 7\ISCC.exe')
        (Join-Path $env:ProgramFiles 'Inno Setup 7\ISCC.exe')
        # winget 无管理员权限时会装到用户目录
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe')
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 7\ISCC.exe')
    ) | Where-Object { $_ -and (Test-Path $_) }

    if (-not $candidates) {
        throw @'
未找到 Inno Setup 编译器 ISCC.exe。
请先安装 Inno Setup 6（https://jrsoftware.org/isdl.php），
或把 ISCC.exe 所在目录加入 PATH 后重试。
'@
    }
    $candidates | Select-Object -First 1
}

function Invoke-TauriBuild {
    if (Get-Command 'pnpm' -ErrorAction SilentlyContinue) {
        pnpm tauri build
    }
    elseif (Get-Command 'npm' -ErrorAction SilentlyContinue) {
        npm run tauri -- build
    }
    else {
        throw '未找到 pnpm 或 npm，请先安装 Node.js 与 pnpm 后重试。'
    }
}

function Get-AppVersion {
    $confPath = Join-Path $TauriDir 'tauri.conf.json'
    if (-not (Test-Path $confPath)) {
        throw "未找到 $confPath"
    }
    (Get-Content $confPath -Raw | ConvertFrom-Json).version
}

# --- 1. 构建 Tauri 应用 -----------------------------------------------------
if (-not $SkipBuild) {
    Write-Host '==> 构建 Tauri 应用（release）' -ForegroundColor Cyan
    Push-Location $DesktopDir
    try {
        Invoke-TauriBuild
        if ($LASTEXITCODE -ne 0) { throw "tauri build 失败（exit=$LASTEXITCODE）" }
    }
    finally {
        Pop-Location
    }
}

# --- 2. 准备待打包文件 ------------------------------------------------------
$mainExe = Join-Path $ReleaseDir $ExeName
if (-not (Test-Path $mainExe)) {
    throw "未找到 $mainExe，请先构建（去掉 -SkipBuild）"
}

Write-Host "==> 准备打包文件 -> $StagingDir" -ForegroundColor Cyan
if (Test-Path $StagingDir) { Remove-Item $StagingDir -Recurse -Force }
New-Item -ItemType Directory -Path $StagingDir -Force | Out-Null
Copy-Item $mainExe -Destination $StagingDir

# 若将来在 tauri.conf.json 里配置了 resources / externalBin，把目录一起带上
$resourcesDir = Join-Path $TauriDir 'resources'
if (Test-Path $resourcesDir) {
    Copy-Item (Join-Path $resourcesDir '*') -Destination $StagingDir -Recurse -Force
}

# --- 3. 编译安装程序 --------------------------------------------------------
$version = Get-AppVersion
$iscc = Get-IsccPath
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null

Write-Host "==> 编译安装程序（Inno Setup $version）" -ForegroundColor Cyan
& $iscc $IssFile "/DMyAppVersion=$version" "/DStagingDir=$StagingDir" "/O$OutputDir" "/FStudyAgent_${version}_x64-setup"
if ($LASTEXITCODE -ne 0) { throw "ISCC 编译失败（exit=$LASTEXITCODE）" }

$setupExe = Join-Path $OutputDir "StudyAgent_${version}_x64-setup.exe"
if (-not (Test-Path $setupExe)) { throw "未生成预期的安装程序：$setupExe" }

$hash = (Get-FileHash $setupExe -Algorithm SHA256).Hash.ToLowerInvariant()
$sizeMb = [math]::Round((Get-Item $setupExe).Length / 1MB, 2)
Write-Host ''
Write-Host "安装程序已生成：$setupExe" -ForegroundColor Green
Write-Host "大小：$sizeMb MB"
Write-Host "SHA-256：$hash"
