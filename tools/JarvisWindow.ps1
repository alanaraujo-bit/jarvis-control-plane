<#
.SYNOPSIS
  Locate the J.A.R.V.I.S. development window, unambiguously.

.DESCRIPTION
  Shared by the capture and key-sending tools.

  These tools send synthetic keystrokes, so a mis-identified window types into
  somebody else's application. Two weaker identifiers were tried and both
  mis-targeted a real, unrelated window on this machine:

   * Window title substring - the project folder is named "j.a.r.v.i.s", so
     editor and terminal windows carry that string in their titles.
   * Process name - this machine also runs an unrelated application whose
     executable is likewise named jarvis.exe.

  The only safe identifier is the full path of the owning executable, which must
  live under the development tree. Use Get-JarvisWindowInfo to inspect what
  would be targeted without touching it.
#>

$script:JarvisWindowTypes = @'
using System;
using System.Runtime.InteropServices;
using System.Text;

public class JarvisWindow {
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc lpEnumFunc, IntPtr lParam);
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern int GetWindowTextLength(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint idAttach, uint idAttachTo, bool fAttach);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr hWnd);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, UIntPtr extra);

    private const uint MOUSEEVENTF_LEFTDOWN = 0x0002;
    private const uint MOUSEEVENTF_LEFTUP   = 0x0004;

    // Click at a point given in window-relative coordinates.
    public static void ClickInWindow(IntPtr hWnd, int x, int y) {
        RECT r = GetFrameBounds(hWnd);
        SetCursorPos(r.Left + x, r.Top + y);
        System.Threading.Thread.Sleep(40);
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(30);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, UIntPtr.Zero);
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left, Top, Right, Bottom; }

    [DllImport("dwmapi.dll")]
    public static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out RECT value, int size);

    // DWMWA_EXTENDED_FRAME_BOUNDS = 9. GetWindowRect would include the
    // invisible resize border and leave a transparent margin on every capture.
    public static RECT GetFrameBounds(IntPtr hwnd) {
        RECT r;
        DwmGetWindowAttribute(hwnd, 9, out r, Marshal.SizeOf(typeof(RECT)));
        return r;
    }

    public static uint ProcessIdOf(IntPtr hWnd) {
        uint pid;
        GetWindowThreadProcessId(hWnd, out pid);
        return pid;
    }

    // Largest visible top-level window belonging to any of the given pids.
    public static IntPtr FindByProcessIds(uint[] pids) {
        IntPtr best = IntPtr.Zero;
        int bestArea = 0;
        EnumWindows(delegate(IntPtr hWnd, IntPtr lParam) {
            if (!IsWindowVisible(hWnd)) return true;
            if (GetWindowTextLength(hWnd) == 0) return true;
            uint pid = ProcessIdOf(hWnd);
            bool mine = false;
            foreach (uint p in pids) { if (p == pid) { mine = true; break; } }
            if (!mine) return true;
            RECT r = GetFrameBounds(hWnd);
            int area = (r.Right - r.Left) * (r.Bottom - r.Top);
            if (area > bestArea) { bestArea = area; best = hWnd; }
            return true;
        }, IntPtr.Zero);
        return best;
    }

    // Raise a window reliably.
    //
    // Windows refuses SetForegroundWindow from a process that does not already
    // own the foreground. Attaching to the current foreground thread's input
    // queue lifts that restriction, which is the documented way to activate a
    // window you own.
    public static bool Activate(IntPtr hWnd) {
        ShowWindow(hWnd, 9); // SW_RESTORE
        if (SetForegroundWindow(hWnd) && GetForegroundWindow() == hWnd) return true;

        IntPtr fg = GetForegroundWindow();
        uint ignoredPid;
        uint fgThread = GetWindowThreadProcessId(fg, out ignoredPid);
        uint thisThread = GetCurrentThreadId();

        AttachThreadInput(thisThread, fgThread, true);
        BringWindowToTop(hWnd);
        SetForegroundWindow(hWnd);
        AttachThreadInput(thisThread, fgThread, false);

        return GetForegroundWindow() == hWnd;
    }
}
'@

if (-not ([System.Management.Automation.PSTypeName]'JarvisWindow').Type) {
  Add-Type -TypeDefinition $script:JarvisWindowTypes
}

function Get-JarvisWindowInfo {
  <#
  .SYNOPSIS
    Report what would be targeted, without activating or touching it.
  #>
  param([string]$ExeRoot = (Split-Path -Parent $PSScriptRoot))

  $root = (Resolve-Path -LiteralPath $ExeRoot).Path

  # Both names: release builds are jarvis-desktop.exe, and cargo still emits
  # jarvis.exe when running the crate directly. The path filter below is what
  # actually guarantees we target our own build.
  $all = @(Get-Process -Name jarvis, jarvis-desktop -ErrorAction SilentlyContinue)
  $candidates = @($all | Where-Object {
    $_.Path -and $_.Path.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)
  })

  if ($candidates.Count -eq 0) {
    $seen = if ($all) { ($all | ForEach-Object { $_.Path }) -join '; ' } else { '(none)' }
    throw "No jarvis.exe running from '$root'. Start the dev app first. Other jarvis processes seen: $seen"
  }

  $procById = @{}
  foreach ($p in $candidates) { $procById[[uint32]$p.Id] = $p }

  $pids = [uint32[]]@($candidates | ForEach-Object { [uint32]$_.Id })
  $hwnd = [JarvisWindow]::FindByProcessIds($pids)
  if ($hwnd -eq [IntPtr]::Zero) {
    throw "The dev jarvis.exe is running but has no visible window yet."
  }

  # Belt and braces: re-verify the path of the window's actual owner. If the
  # lookup above ever regresses, this is what stops keystrokes from leaving.
  $ownerPid = [JarvisWindow]::ProcessIdOf($hwnd)
  $owner = $procById[[uint32]$ownerPid]
  if (-not $owner -or -not $owner.Path.StartsWith($root, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to target window $hwnd - its owning process is not the dev build."
  }

  return [pscustomobject]@{
    Hwnd      = $hwnd
    ProcessId = [int]$ownerPid
    Path      = $owner.Path
  }
}

function Set-JarvisFocus {
  param([IntPtr]$Hwnd, [int]$Attempts = 6)

  for ($i = 0; $i -lt $Attempts; $i++) {
    if ([JarvisWindow]::Activate($Hwnd)) { return }
    Start-Sleep -Milliseconds 250
  }
  throw "Could not bring the J.A.R.V.I.S. window to the foreground."
}

function Assert-JarvisFocused {
  param([IntPtr]$Hwnd, [int]$ProcessId)

  # Never send keystrokes unless the foreground window belongs to the dev app.
  # A missed activation must fail loudly rather than type into another
  # application.
  #
  # The check is on the owning process, not on the exact handle: opening a file
  # dialog moves focus to a different window of the *same* process, and driving
  # that dialog is legitimate. Anything owned by another process is not.
  $fg = [JarvisWindow]::GetForegroundWindow()
  if ($fg -eq $Hwnd) { return }

  if ($ProcessId -and [JarvisWindow]::ProcessIdOf($fg) -eq [uint32]$ProcessId) { return }

  throw "Refusing to send input: the foreground window does not belong to the J.A.R.V.I.S. dev app."
}
