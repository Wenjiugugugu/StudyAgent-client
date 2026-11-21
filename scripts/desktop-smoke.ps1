param([string]$Executable = "src-tauri/target/release/studyagent-desktop.exe", [string]$ReportPath = "")
$ErrorActionPreference = "Stop"
$resolvedExe = (Resolve-Path -LiteralPath $Executable).Path
$smokeTempBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$smokeRoot = Join-Path $smokeTempBase ("studyagent-desktop-smoke-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $smokeRoot | Out-Null
Set-Content -LiteralPath (Join-Path $smokeRoot ".studyagent-smoke-root") -Value "isolated test"
$result = $null
$process = Start-Process -FilePath $resolvedExe -ArgumentList @("--desktop-smoke-root", ('"' + $smokeRoot + '"')) -WindowStyle Hidden -PassThru
try {
  if (-not $process.WaitForExit(45000)) { Stop-Process -Id $process.Id; throw "Desktop smoke timed out" }
  $report = Join-Path $smokeRoot "desktop-smoke-result.json"
  if (-not (Test-Path -LiteralPath $report)) { throw "Desktop did not report frontend readiness: $smokeRoot" }
  $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
  if ($process.ExitCode -ne 0 -or -not $result.ok) { throw ("Desktop smoke failed: " + ($result | ConvertTo-Json -Compress -Depth 5)) }
  if ($ReportPath) { $result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $ReportPath -Encoding utf8 }
  Write-Output ("Desktop frontend mounted and IPC read/write passed: " + $report)
} finally {
  # 失败保留日志；只清理经过路径校验的成功测试目录。
  if ($result -and $result.ok) {
    $checkedRoot = [System.IO.Path]::GetFullPath($smokeRoot)
    if (-not $checkedRoot.StartsWith($smokeTempBase, [StringComparison]::OrdinalIgnoreCase) -or $checkedRoot -eq $smokeTempBase) { throw "Unsafe cleanup target" }
    Remove-Item -LiteralPath $checkedRoot -Recurse -Force
  }
}
