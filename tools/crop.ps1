<#
.SYNOPSIS
  Crop and downscale a screenshot for the documentation site.

.DESCRIPTION
  A 1442x902 window photograph rendered into a 750px reading column is 52% of
  its captured size, and its 13.5px interface text lands at about 7px — present
  but unreadable, which is worse than absent. Figures are therefore cropped to
  the part being written about and only then scaled.

  -Crop is "x,y,w,h" in captured pixels; omit it to keep the whole frame.
  -Width is the output width; the height follows the aspect ratio.
#>
param(
  [Parameter(Mandatory = $true)][string]$In,
  [Parameter(Mandatory = $true)][string]$Out,
  [string]$Crop = "",
  [int]$Width = 0
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$src = [System.Drawing.Image]::FromFile((Resolve-Path $In).Path)
try {
  if ($Crop) {
    $p = $Crop.Split(',') | ForEach-Object { [int]$_.Trim() }
    $rect = New-Object System.Drawing.Rectangle $p[0], $p[1], $p[2], $p[3]
  } else {
    $rect = New-Object System.Drawing.Rectangle 0, 0, $src.Width, $src.Height
  }

  $cropped = New-Object System.Drawing.Bitmap $rect.Width, $rect.Height
  $g = [System.Drawing.Graphics]::FromImage($cropped)
  $g.DrawImage($src, (New-Object System.Drawing.Rectangle 0, 0, $rect.Width, $rect.Height), $rect, [System.Drawing.GraphicsUnit]::Pixel)
  $g.Dispose()

  if ($Width -gt 0 -and $Width -ne $rect.Width) {
    $h = [int][Math]::Round($rect.Height * ($Width / $rect.Width))
    $scaled = New-Object System.Drawing.Bitmap $Width, $h
    $g2 = [System.Drawing.Graphics]::FromImage($scaled)
    $g2.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g2.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g2.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
    $g2.DrawImage($cropped, 0, 0, $Width, $h)
    $g2.Dispose()
    $cropped.Dispose()
    $cropped = $scaled
  }

  $dir = Split-Path -Parent $Out
  if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
  # .NET resolves a relative path against its own working directory, not the
  # shell's, so a relative -Out silently writes somewhere else or fails.
  $full = if ($dir) { Join-Path (Resolve-Path $dir).Path (Split-Path -Leaf $Out) } else { Join-Path (Get-Location).Path $Out }
  $cropped.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Output "$full : $($cropped.Width)x$($cropped.Height)"
  $cropped.Dispose()
} finally {
  $src.Dispose()
}
