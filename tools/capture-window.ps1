<#
.SYNOPSIS
  Capture a real screenshot of a running window.

.DESCRIPTION
  Visual review has to look at the actual application, not at a browser
  approximation of it (§76). This finds a top-level window by title substring,
  brings it forward, and writes a PNG of exactly its frame.

  Uses DWMWA_EXTENDED_FRAME_BOUNDS rather than GetWindowRect: on Windows 10+
  GetWindowRect includes the invisible resize border, which would leave a
  transparent margin around every capture.

.EXAMPLE
  ./capture-window.ps1 -Title "J.A.R.V.I.S." -Out shots/mission-control.png
#>
param(
  [Parameter(Mandatory = $true)][string]$Title,
  [Parameter(Mandatory = $true)][string]$Out,
  [int]$SettleMs = 700
)

Add-Type -AssemblyName System.Drawing

$signature = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class WinCapture {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT value, int size);

    // DWMWA_EXTENDED_FRAME_BOUNDS = 9
    public static RECT GetFrameBounds(IntPtr hwnd) {
        RECT r;
        DwmGetWindowAttribute(hwnd, 9, out r, Marshal.SizeOf(typeof(RECT)));
        return r;
    }

    public static IntPtr FindByTitle(string needle) {
        IntPtr found = IntPtr.Zero;
        EnumWindows(delegate(IntPtr hWnd, IntPtr lParam) {
            if (!IsWindowVisible(hWnd)) return true;
            int len = GetWindowTextLength(hWnd);
            if (len == 0) return true;
            StringBuilder sb = new StringBuilder(len + 1);
            GetWindowText(hWnd, sb, sb.Capacity);
            if (sb.ToString().IndexOf(needle, StringComparison.OrdinalIgnoreCase) >= 0) {
                found = hWnd;
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@

if (-not ([System.Management.Automation.PSTypeName]'WinCapture').Type) {
  Add-Type -TypeDefinition $signature -ReferencedAssemblies System.Drawing, System.Runtime.InteropServices
}

$hwnd = [WinCapture]::FindByTitle($Title)
if ($hwnd -eq [IntPtr]::Zero) {
  Write-Error "No visible window matching '$Title'."
  exit 1
}

# SW_RESTORE, then raise, then let the compositor settle before reading pixels.
[void][WinCapture]::ShowWindow($hwnd, 9)
[void][WinCapture]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds $SettleMs

$r = [WinCapture]::GetFrameBounds($hwnd)
$width = $r.Right - $r.Left
$height = $r.Bottom - $r.Top

if ($width -le 0 -or $height -le 0) {
  Write-Error "Window reported a zero-size frame ($width x $height)."
  exit 1
}

$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($r.Left, $r.Top, 0, 0, $bitmap.Size)

$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }

$bitmap.Save((Resolve-Path -LiteralPath (Split-Path -Parent $Out)).Path + "\" + (Split-Path -Leaf $Out), [System.Drawing.Imaging.ImageFormat]::Png)
$graphics.Dispose()
$bitmap.Dispose()

Write-Output "captured ${width}x${height} -> $Out"
