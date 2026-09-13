#!/usr/bin/env pwsh
<#
.SYNOPSIS
    构建 StudyAgent 的 Windows 安装程序（Tauri release + Inno Setup）。

.DESCRIPTION
    流程：
      1. 用「持久编译缓存目录」执行 Tauri release 构建（产物 studyagent-desktop.exe）
      2. 把待打包文件复制到 installer\staging
      3. 调用 ISCC.exe 编译 installer\StudyAgent.iss
    产物：
      src-tauri\target\release\bundle\inno\StudyAgent_<version>_x64-setup.exe

    增量编译说明（重要）：
      - 本机 `desktop\src-tauri\target` 会被安全软件随机拦截写入（cargo 报 os error 5），
        直接写它会构建失败并废掉缓存。因此脚本把 cargo 编译目录（CARGO_TARGET_DIR）
        固定到一个持久的缓存目录，绕开被拦截路径。
      - 默认缓存目录：%LOCALAPPDATA%\StudyAgent\tauri-target（首次使用会全量编译，
        约 15-22 分钟）；之后再次执行本脚本，cargo 会自动增量，只重编改动部分。
      - 只改了前端（Vue）时：先确保 dist 已更新（或保留默认前端构建步骤），
        再跑本脚本，约 1-3 分钟即可出包。
      - 已存在的缓存目录可直接复用，避免重新全量编译，例如：
        .\installer\build.ps1 -TargetDir 'D:\c\Users\Administrator\AppData\Local\Temp\tauri-target-070'
      - 目录可通过环境变量 STUDYAGENT_TARGET_DIR 统一指定（优先级低于 -TargetDir）。

.PARAMETER SkipBuild
    跳过 Tauri 构建，仅用缓存目录 release 里已有的 studyagent-desktop.exe 重新打包
    （适合只重打安装程序）。

.PARAMETER TargetDir
    cargo 编译缓存目录（CARGO_TARGET_DIR）。默认 %LOCALAPPDATA%\StudyAgent\tauri-target；
    可用环境变量 STUDYAGENT_TARGET_DIR 覆盖默认值。

.PARAMETER SkipFrontend
    跳过 tauri 的 beforeBuildCommand（前端 vue-tsc + vite build）。当 dist 已是最新、
    只想快速重编嵌入时使用；否则默认会先重建前端（需要 npm 可用）。

.EXAMPLE
    # 完整打包（含前端重建；首次会全量编译 Rust）
    .\installer\build.ps1

    # 前端已就绪，跳过前端构建，增量重编 Rust 后打安装包
    .\installer\build.ps1 -SkipFrontend

    # 复用已有缓存目录（避免全量），其余同默认
    .\installer\build.ps1 -TargetDir 'D:\c\Users\Administrator\AppData\Local\Temp\tauri-target-070'

    # 仅用已有 exe 重打安装程序
    .\installer\build.ps1 -SkipBuild
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$TargetDir = '',
    [switch]$SkipFrontend
)

$ErrorActionPreference = 'Stop'

$InstallerDir = $PSScriptRoot
$DesktopDir = Split-Path -Parent $InstallerDir
$TauriDir = Join-Path $DesktopDir 'src-tauri'
$StagingDir = Join-Path $InstallerDir 'staging'
# 安装包输出目录维持既有发布习惯（与 tauri bundle 默认一致）
$OutputDir = Join-Path $TauriDir 'target\release\bundle\inno'
$IssFile = Join-Path $InstallerDir 'StudyAgent.iss'
$ExeName = 'studyagent-desktop.exe'

# ── 编译缓存目录（CARGO_TARGET_DIR）──────────────────────────────────────
# 优先级：-TargetDir > 环境变量 STUDYAGENT_TARGET_DIR > LOCALAPPDATA 默认
if ([string]::IsNullOrWhiteSpace($TargetDir)) {
    $TargetDir = $env:STUDYAGENT_TARGET_DIR
}
if ([string]::IsNullOrWhiteSpace($TargetDir)) {
    $TargetDir = Join-Path $env:LOCALAPPDATA 'StudyAgent\tauri-target'
}
# 把 Windows 风格路径转成 cargo/rustc 认识的格式（避免正斜杠盘符歧义）
$TargetDir = $TargetDir -replace '/', '\'
$ReleaseDir = Join-Path $TargetDir 'release'
$MainExe = Join-Path $ReleaseDir $ExeName

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

function Get-AppVersion {
    $confPath = Join-Path $TauriDir 'tauri.conf.json'
    if (-not (Test-Path $confPath)) {
        throw "未找到 $confPath"
    }
    (Get-Content $confPath -Raw | ConvertFrom-Json).version
}

# ── 1. 构建 Tauri 应用（release，写持久缓存目录）───────────────────────
if (-not $SkipBuild) {
    New-Item -ItemType Directory -Path $TargetDir -Force | Out-Null
    $env:CARGO_TARGET_DIR = $TargetDir
    Write-Host "==> CARGO_TARGET_DIR = $TargetDir" -ForegroundColor Cyan

    # 优先使用仓库自带的 tauri CLI（node_modules），避免依赖全局 pnpm/npm
    $tauriCli = Join-Path $DesktopDir 'node_modules\@tauri-apps\cli\tauri.js'
    $nodeExe = (Get-Command 'node.exe' -ErrorAction SilentlyContinue).Source
    if (-not $nodeExe) { $nodeExe = (Get-Command 'node' -ErrorAction SilentlyContinue).Source }
    if (-not (Test-Path $tauriCli)) {
        throw "未找到仓库内置 tauri CLI：$tauriCli（请先在 desktop 目录执行依赖安装）"
    }
    if (-not $nodeExe) {
        throw '未找到 node，请先安装 Node.js 或把 node.exe 所在目录加入 PATH。'
    }

    $tauriArgs = @('build', '--no-bundle')
    if ($SkipFrontend -or -not (Get-Command 'npm' -ErrorAction SilentlyContinue)) {
        # 跳过 beforeBuildCommand（npm run build）：要求 desktop/dist 已是最新
        if ($SkipFrontend) {
            Write-Host '==> 已跳过前端构建（-SkipFrontend），使用现有 desktop/dist' -ForegroundColor Yellow
        } else {
            Write-Host '==> 未检测到 npm，使用现有 desktop/dist（如非最新请先构建前端）' -ForegroundColor Yellow
        }
        $tauriArgs += @('--config', '{"build":{"beforeBuildCommand":""}}')
    }

    Write-Host '==> 构建 Tauri 应用（release，增量缓存）' -ForegroundColor Cyan
    Push-Location $DesktopDir
    try {
        & $nodeExe $tauriCli @tauriArgs
        if ($LASTEXITCODE -ne 0) { throw "tauri build 失败（exit=$LASTEXITCODE）" }
    }
    finally {
        Pop-Location
    }
}

# ── 2. 准备待打包文件 ------------------------------------------------------
if (-not (Test-Path $MainExe)) {
    throw "未找到 $MainExe`n请先执行完整构建（不要加 -SkipBuild），或确认 -TargetDir 指向含 release 产物的缓存目录。"
}

Write-Host "==> 准备打包文件 -> $StagingDir" -ForegroundColor Cyan
if (Test-Path $StagingDir) { Remove-Item $StagingDir -Recurse -Force }
New-Item -ItemType Directory -Path $StagingDir -Force | Out-Null
Copy-Item $MainExe -Destination $StagingDir

# 若将来在 tauri.conf.json 里配置了 resources / externalBin，把目录一起带上
$resourcesDir = Join-Path $TauriDir 'resources'
if (Test-Path $resourcesDir) {
    Copy-Item (Join-Path $resourcesDir '*') -Destination $StagingDir -Recurse -Force
}

# ── 3. 编译安装程序 --------------------------------------------------------
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
