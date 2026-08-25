<#
.SYNOPSIS
  Replace personal data in a documentation screenshot with a placeholder.

.DESCRIPTION
  The Accounts surface prints the email address each provider configuration
  directory is signed in as. That is the right thing for the product to show
  and the wrong thing to publish, so the figures that carry it are anonymised
  before they reach the site — and the figure's own caption says they were.

  -Boxes is a list of "x,y,w,h[:text]" in the image's own pixels. Each box is
  painted with the sampled background colour immediately left of it, and the
  optional text is drawn back in the same size and colour as what it replaced.
#>
param(
  [Parameter(Mandatory = $true)][string]$In,
  [Parameter(Mandatory = $true)][string]$Out,
  [Parameter(Mandatory = $true)][string[]]$Boxes,
  [int]$FontSize = 13,
  [string]$Colour = "#9E9EA6",
  [string]$Font = "Segoe UI"
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$src = [System.Drawing.Bitmap]::new((Resolve-Path $In).Path)
try {
  $g = [System.Drawing.Graphics]::FromImage($src)
  $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
  $ink = [System.Drawing.ColorTranslator]::FromHtml($Colour)
  $brush = New-Object System.Drawing.SolidBrush $ink
  $typeface = New-Object System.Drawing.Font $Font, $FontSize, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)

  foreach ($box in $Boxes) {
    $parts = $box.Split(':', 2)
    $n = $parts[0].Split(',') | ForEach-Object { [int]$_.Trim() }
    $x = $n[0]; $y = $n[1]; $w = $n[2]; $h = $n[3]

    # Sample the surface just left of the box rather than assuming a token
    # value: these cards sit on three different backgrounds.
    $sample = $src.GetPixel([Math]::Max(0, $x - 3), $y + [int]($h / 2))
    $g.FillRectangle((New-Object System.Drawing.SolidBrush $sample), $x, $y, $w, $h)

    if ($parts.Length -eq 2 -and $parts[1]) {
      $g.DrawString($parts[1], $typeface, $brush, [float]$x, [float]($y + ($h - $FontSize * 1.35) / 2))
    }
  }

  $g.Dispose()
  $dir = Split-Path -Parent $Out
  if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
  $full = Join-Path (Resolve-Path $dir).Path (Split-Path -Leaf $Out)
  $src.Save($full, [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Output "$full : redacted $($Boxes.Count) region(s)"
} finally {
  $src.Dispose()
}
