<#
.SYNOPSIS
  justdown installer/updater for Windows — download a prebuilt `jd` binary for
  this host from the latest GitHub Release, verify its checksum, install it, and
  put it on PATH. Re-run any time to update to the latest release.

  irm https://raw.githubusercontent.com/yesitsfebreeze/justdown/main/scripts/install.ps1 | iex

  Env: JD_INSTALL_DIR (default $HOME\.local\bin) · JD_VERSION (default: latest tag).
#>
$ErrorActionPreference = 'Stop'

$Repo = 'yesitsfebreeze/justdown'
$Dest = if ($env:JD_INSTALL_DIR) { $env:JD_INSTALL_DIR } else { Join-Path $HOME '.local\bin' }

function Die($msg) { Write-Error "install: $msg"; exit 1 }

$arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
switch ($arch) {
  'AMD64' { $target = 'x86_64-pc-windows-msvc' }
  default { Die "unsupported architecture: $arch — build from source: cargo install --git https://github.com/$Repo jd" }
}

# resolve the version tag (latest unless pinned via JD_VERSION)
$tag = $env:JD_VERSION
if (-not $tag) {
  $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" -TimeoutSec 30
  $tag = $rel.tag_name
  if (-not $tag) { Die 'could not resolve the latest release tag' }
}

$archive = "jd-$tag-$target.zip"
$base    = "https://github.com/$Repo/releases/download/$tag"
$tmp     = Join-Path ([IO.Path]::GetTempPath()) ("jd-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp -Force | Out-Null
try {
  $zip = Join-Path $tmp $archive
  Write-Host "install: fetching $archive ($tag)"
  Invoke-WebRequest -Uri "$base/$archive" -OutFile $zip -TimeoutSec 120

  # verify the checksum against the release's SHA256SUMS (best-effort)
  try {
    $sums = (Invoke-WebRequest -Uri "$base/SHA256SUMS" -TimeoutSec 30).Content
    $want = ($sums -split "`n" | Where-Object { $_ -match [Regex]::Escape($archive) + '$' } |
             ForEach-Object { ($_ -split '\s+')[0] } | Select-Object -First 1)
    if ($want) {
      $got = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLower()
      if ($want.ToLower() -ne $got) { Die "checksum mismatch for $archive" }
      Write-Host 'install: checksum ok'
    }
  } catch {
    Write-Warning 'install: SHA256SUMS not found, skipping verification'
  }

  Expand-Archive -Path $zip -DestinationPath $tmp -Force
  $exe = Join-Path $tmp 'jd.exe'
  if (-not (Test-Path $exe)) { Die 'archive did not contain jd.exe' }
  New-Item -ItemType Directory -Path $Dest -Force | Out-Null
  Copy-Item -Path $exe -Destination (Join-Path $Dest 'jd.exe') -Force
  Write-Host "install: jd $tag -> $(Join-Path $Dest 'jd.exe')"

  # ensure $Dest is on the user PATH so `jd` works from any new shell
  $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (($userPath -split ';') -notcontains $Dest) {
    $newPath = if ($userPath) { "$userPath;$Dest" } else { $Dest }
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Host "install: added $Dest to your PATH — open a new shell to pick it up"
  }
} finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
