$ErrorActionPreference = 'Stop'
$root = 'D:\StudyAgent\desktop'
$items = @(
  @{ Id='6e2e'; Name='task-progress' },
  @{ Id='4b87'; Name='plan-adjustment' },
  @{ Id='0273'; Name='light-review' }
)
foreach ($item in $items) {
  $path = "C:\Users\Administrator\.codex\worktrees\$($item.Id)\desktop"
  $env:GIT_INDEX_FILE = "$root\output\integration-0.8.1\$($item.Id).index"
  git -C $path read-tree HEAD
  if ($LASTEXITCODE) { throw 'read-tree failed' }
  git -C $path add -u
  $untracked = @(git -C $path ls-files --others --exclude-standard)
  foreach ($file in $untracked) { git -C $path add -- $file; if ($LASTEXITCODE) { throw "add failed: $file" } }
  $tree = git -C $path write-tree
  $parent = git -C $path rev-parse HEAD
  $commit = git -C $path commit-tree $tree -p $parent -m "feat: snapshot $($item.Name) for 0.8.1 integration"
  if ($LASTEXITCODE) { throw 'snapshot failed' }
  git -C $root update-ref "refs/heads/codex/0.8.1-$($item.Name)" $commit
  if ($LASTEXITCODE) { throw 'update-ref failed' }
  Write-Output "$($item.Id): $commit"
}
Remove-Item Env:\GIT_INDEX_FILE
Set-Location $root
git switch -c codex/release-0.8.1
if ($LASTEXITCODE) { throw 'switch failed' }
git merge --no-ff codex/0.8.1-task-progress -m 'merge: integrate task knowledge progress for 0.8.1'
if ($LASTEXITCODE) { throw 'merge requires resolution' }
