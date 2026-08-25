<#
.SYNOPSIS
  Drive the application to a surface and photograph it, in one call.

.DESCRIPTION
  Written for the documentation site: every figure in `apps/docs` is a real
  photograph of the real product, so the shot list is a script rather than a
  memory of what somebody clicked once.

  Steps use the same vocabulary as send-keys.ps1 ("click:x,y", "sleep:ms",
  literal SendKeys), and the same safety rule: the window is resolved by the
  path of its owning executable, because this machine carries an unrelated
  jarvis.exe that window-title and process-name matching both hit.

.EXAMPLE
  .\tools\shoot.ps1 -Name accounts -Steps "click:26,278|sleep:1400"
#>
param(
  [Parameter(Mandatory = $true)][string]$Name,
  [string]$Steps = "",
  [string]$OutDir = "$PSScriptRoot\..\.tmp\docshots",
  [string]$ExeRoot = "$PSScriptRoot\..",
  [int]$SettleMs = 900
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'JarvisWindow.ps1')
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing

$info = Get-JarvisWindowInfo -ExeRoot $ExeRoot
Set-JarvisFocus -Hwnd $info.Hwnd
Start-Sleep -Milliseconds 400
Assert-JarvisFocused -Hwnd $info.Hwnd -ProcessId $info.ProcessId

foreach ($step in $Steps.Split('|')) {
  if ([string]::IsNullOrEmpty($step)) { continue }
  if ($step -match '^sleep:(\d+)$') { Start-Sleep -Milliseconds ([int]$Matches[1]); continue }
  Assert-JarvisFocused -Hwnd $info.Hwnd -ProcessId $info.ProcessId
  if ($step -match '^click:(-?\d+),(-?\d+)$') {
    [JarvisWindow]::ClickInWindow($info.Hwnd, [int]$Matches[1], [int]$Matches[2])
    Start-Sleep -Milliseconds 220
    continue
  }
  [System.Windows.Forms.SendKeys]::SendWait($step)
  Start-Sleep -Milliseconds 90
}

# Park the pointer somewhere inert before the shutter. Leaving it where the
# last click landed makes the rail draw its tooltip and every button under it
# sit in its hover state — a photograph of the product being poked rather than
# of the product.
$park = [JarvisWindow]::GetFrameBounds($info.Hwnd)
[JarvisWindow]::SetCursorPos($park.Right - 4, $park.Bottom - 3) | Out-Null

# Let animation finish. Motion here is decelerate-only and short (§10), but a
# capture taken mid-transition documents a state the product never rests in.
Start-Sleep -Milliseconds $SettleMs
Assert-JarvisFocused -Hwnd $info.Hwnd -ProcessId $info.ProcessId

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force $OutDir | Out-Null }
$out = Join-Path (Resolve-Path $OutDir).Path "$Name.png"

$r = [JarvisWindow]::GetFrameBounds($info.Hwnd)
$w = $r.Right - $r.Left
$h = $r.Bottom - $r.Top
if ($w -le 0 -or $h -le 0) { throw "Window reported a zero-size frame." }

$bmp = New-Object System.Drawing.Bitmap $w, $h
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($r.Left, $r.Top, 0, 0, $bmp.Size)
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose()
$bmp.Dispose()

Write-Output "$Name : ${w}x${h} -> $out"
