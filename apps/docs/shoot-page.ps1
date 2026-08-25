<#
.SYNOPSIS
  Render a built documentation page headlessly and save a screenshot.

.DESCRIPTION
  Visual review of this site is done the same way visual review of the product
  is: by looking at the real thing. Headless Edge renders the built page from
  `file://` — which also proves the site works from disk, not only from a
  server.

  -Theme forces the stored preference; omit it to get whatever
  prefers-color-scheme decides.

.EXAMPLE
  .\shoot-page.ps1 -Page en/overview.html -Width 1440 -Height 2200 -Out over.png
#>
param(
  [Parameter(Mandatory = $true)][string]$Page,
  [int]$Width = 1440,
  [int]$Height = 1800,
  [ValidateSet("", "dark", "light")][string]$Theme = "",
  [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = 'Stop'
$edge = "C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
if (-not (Test-Path $edge)) { $edge = "C:\Program Files\Microsoft\Edge\Application\msedge.exe" }

$dist = Join-Path $PSScriptRoot "dist"
$file = Join-Path $dist $Page
if (-not (Test-Path $file)) { throw "not built: $Page" }

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
$outFull = Join-Path (Resolve-Path ($dir ? $dir : ".")).Path (Split-Path -Leaf $Out)

$url = "file:///" + ((Resolve-Path $file).Path -replace '\\', '/')
if ($Theme) { $url += "?theme=$Theme" }

$edgeArgs = @(
  "--headless=new", "--disable-gpu", "--hide-scrollbars",
  "--window-size=$Width,$Height",
  "--screenshot=$outFull",
  # A profile of its own, so localStorage from one run cannot leak into the
  # next and quietly make a theme check pass for the wrong reason.
  "--user-data-dir=$env:TEMP\jarvis-docshot-$Theme",
  $url
)

# $args is an automatic PowerShell variable — assigning to it silently
# does not do what it looks like it does, and the URL loses its query string.
& $edge @edgeArgs 2>&1 | Select-Object -Last 1 | Out-Null
if (-not (Test-Path $outFull)) { throw "no screenshot written for $Page" }
Write-Output "$Page ($Width x $Height$(if ($Theme) { ", $Theme" })) -> $outFull"
