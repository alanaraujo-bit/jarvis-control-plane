<#
.SYNOPSIS
  Drive the J.A.R.V.I.S. development window with real keystrokes.

.DESCRIPTION
  Exercises the product the way a person would, through the real UI (§76).

  Safety rules, all learned the hard way on this machine:
   * the window is resolved by owning-executable path, never by title or
     process name (both mis-targeted unrelated windows);
   * focus is re-asserted before every single step, so a stolen foreground
     aborts instead of typing into somebody else's application.

  Steps are separated by "|" and are either literal SendKeys strings or
  "sleep:<ms>".
#>
param(
  [Parameter(Mandatory = $true)][string]$Steps,
  [string]$ExeRoot = (Split-Path -Parent $PSScriptRoot),
  [int]$FocusSettleMs = 500
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'JarvisWindow.ps1')
Add-Type -AssemblyName System.Windows.Forms

$info = Get-JarvisWindowInfo -ExeRoot $ExeRoot
Write-Output "targeting pid $($info.ProcessId): $($info.Path)"

Set-JarvisFocus -Hwnd $info.Hwnd
Start-Sleep -Milliseconds $FocusSettleMs
Assert-JarvisFocused -Hwnd $info.Hwnd -ProcessId $info.ProcessId

foreach ($step in $Steps.Split('|')) {
  if ($step -match '^sleep:(\d+)$') { Start-Sleep -Milliseconds ([int]$Matches[1]); continue }

  # "click:x,y" clicks a point relative to the window frame, so the UI can be
  # driven the way a person drives it rather than through internals.
  if ($step -match '^click:(\d+),(\d+)$') {
    Assert-JarvisFocused -Hwnd $info.Hwnd -ProcessId $info.ProcessId
    [JarvisWindow]::ClickInWindow($info.Hwnd, [int]$Matches[1], [int]$Matches[2])
    Start-Sleep -Milliseconds 150
    continue
  }

  if ([string]::IsNullOrEmpty($step)) { continue }
  Assert-JarvisFocused -Hwnd $info.Hwnd -ProcessId $info.ProcessId
  [System.Windows.Forms.SendKeys]::SendWait($step)
  Start-Sleep -Milliseconds 70
}

Write-Output "sent steps to pid $($info.ProcessId)"
