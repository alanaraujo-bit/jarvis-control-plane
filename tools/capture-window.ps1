<#
.SYNOPSIS
  Capture a real screenshot of the J.A.R.V.I.S. development window.

.DESCRIPTION
  Visual review must look at the actual application (§76). The window is
  resolved by the path of its owning executable - see JarvisWindow.ps1.
#>
param(
  [Parameter(Mandatory = $true)][string]$Out,
  [string]$ExeRoot = (Split-Path -Parent $PSScriptRoot),
  [int]$SettleMs = 600
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'JarvisWindow.ps1')
Add-Type -AssemblyName System.Drawing

$info = Get-JarvisWindowInfo -ExeRoot $ExeRoot
Set-JarvisFocus -Hwnd $info.Hwnd
Start-Sleep -Milliseconds $SettleMs

# The capture reads the screen at the window's coordinates, so it is only
# trustworthy if that window is genuinely on top.
Assert-JarvisFocused -Hwnd $info.Hwnd -ProcessId $info.ProcessId

$r = [JarvisWindow]::GetFrameBounds($info.Hwnd)
$width = $r.Right - $r.Left
$height = $r.Bottom - $r.Top
if ($width -le 0 -or $height -le 0) { throw "Window reported a zero-size frame." }

$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($r.Left, $r.Top, 0, 0, $bitmap.Size)

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
$full = Join-Path (Resolve-Path $dir).Path (Split-Path -Leaf $Out)

$bitmap.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()
Write-Output "captured ${width}x${height} from pid $($info.ProcessId) -> $Out"
