# Phase 0 acceptance (post-rework): verify the TWO-button window controls
# (PCL2 FormMain has no maximize button; double-click on the drag region
# toggles maximize/restore instead).
# Flow: foreground window (ALT unlock + AttachThreadInput, see phase log) ->
#       click minimize (assert IsIconic) -> restore ->
#       double-click drag region (assert IsZoomed) -> double-click again
#       (assert restored size) -> click close (assert process exits).
# Usage (repo root): & .\scripts\verify-window-controls.ps1 -ProcessName "dsh-launcher"
# WARNING: the final step CLOSES the app. Run only when that is acceptable.
# NOTE: keep this file ASCII-only; it is read by Windows PowerShell without a BOM.
param(
  [Parameter(Mandatory = $true)][string]$ProcessName
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class WinVerify {
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);

  // Largest visible top-level window of the process = the app main window.
  public static IntPtr FindMainByPid(uint targetPid) {
    IntPtr best = IntPtr.Zero;
    long bestArea = -1;
    EnumWindows((hWnd, lParam) => {
      uint pid; GetWindowThreadProcessId(hWnd, out pid);
      if (pid == targetPid && IsWindowVisible(hWnd)) {
        RECT r; GetWindowRect(hWnd, out r);
        long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
        if (area > bestArea) { bestArea = area; best = hWnd; }
      }
      return true;
    }, IntPtr.Zero);
    return best;
  }

  // Background processes cannot take foreground via SetForegroundWindow alone
  // (Windows foreground lock). ALT tap + AttachThreadInput unlocks it.
  public static bool ForceForeground(IntPtr hWnd) {
    keybd_event(0x12, 0, 0, UIntPtr.Zero); keybd_event(0x12, 0, 2, UIntPtr.Zero);
    uint dummy = 0;
    uint foreThread = GetWindowThreadProcessId(GetForegroundWindow(), out dummy);
    uint thisThread = GetCurrentThreadId();
    uint targetThread = GetWindowThreadProcessId(hWnd, out dummy);
    bool attached = false;
    if (foreThread != thisThread && foreThread != 0) {
      AttachThreadInput(thisThread, foreThread, true);
      AttachThreadInput(thisThread, targetThread, true);
      attached = true;
    }
    bool ok = SetForegroundWindow(hWnd);
    if (attached) {
      AttachThreadInput(thisThread, foreThread, false);
      AttachThreadInput(thisThread, targetThread, false);
    }
    return ok && GetForegroundWindow() == hWnd;
  }
}
"@

[void][WinVerify]::SetProcessDPIAware()

$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { throw "process not found: $ProcessName" }
$hwnd = [WinVerify]::FindMainByPid([uint32]$proc.Id)
if ($hwnd -eq [IntPtr]::Zero) { throw "no visible top-level window for pid $($proc.Id)" }

function Get-Rect([IntPtr]$h) {
  $r = New-Object WinVerify+RECT
  [void][WinVerify]::GetWindowRect($h, [ref]$r)
  return $r
}

function Invoke-ClickAt([int]$x, [int]$y, [int]$doubleDelayMs = 90) {
  [void][WinVerify]::SetCursorPos($x, $y)
  Start-Sleep -Milliseconds 150
  [WinVerify]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)  # LEFTDOWN
  Start-Sleep -Milliseconds 70
  [WinVerify]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)  # LEFTUP
}

function Invoke-DoubleClickAt([int]$x, [int]$y) {
  Invoke-ClickAt $x $y
  Start-Sleep -Milliseconds 90   # within system double-click time
  Invoke-ClickAt $x $y
  Start-Sleep -Milliseconds 700
}

$g = [System.Drawing.Graphics]::FromHwnd([IntPtr]::Zero)
$scale = $g.DpiX / 96.0
$g.Dispose()

if (-not [WinVerify]::ForceForeground($hwnd)) { throw "could not take foreground for hwnd $hwnd" }
[void][WinVerify]::ShowWindow($hwnd, 9)
Start-Sleep -Milliseconds 600

$results = [ordered]@{}

# Geometry: topbar is 48 logical px tall -> vertical center at +24 logical.
# Two buttons flush right, each ~28 logical px wide (PCL2 FormMain):
#   close  center at Right - 12 - 14 = Right - 26 logical
#   min    center at Right - 44 - 14 = Right - 58 logical
# Drag-region double-click point: topbar center-left, away from buttons/text.
function Get-BtnY([IntPtr]$h) {
  $r = Get-Rect $h
  return [int]($r.Top + 24 * $scale)
}
function Get-CloseX([IntPtr]$h) {
  $r = Get-Rect $h
  return [int]($r.Right - 26 * $scale)
}
function Get-MinX([IntPtr]$h) {
  $r = Get-Rect $h
  return [int]($r.Right - 58 * $scale)
}
function Get-DragX([IntPtr]$h) {
  $r = Get-Rect $h
  return [int]($r.Left + 120 * $scale)
}

# 1) minimize
Invoke-ClickAt (Get-MinX $hwnd) (Get-BtnY $hwnd)
Start-Sleep -Milliseconds 400
if ([WinVerify]::IsIconic($hwnd)) { $results["minimize"] = "PASS IsIconic=True" }
else { $results["minimize"] = "FAIL IsIconic=False" }

# 2) restore from taskbar state
[void][WinVerify]::ShowWindow($hwnd, 9)
Start-Sleep -Milliseconds 500

# 3) double-click drag region -> maximize
$r0 = Get-Rect $hwnd
Invoke-DoubleClickAt (Get-DragX $hwnd) (Get-BtnY $hwnd)
$r1 = Get-Rect $hwnd
if ([WinVerify]::IsZoomed($hwnd)) { $results["dblclick-maximize"] = "PASS IsZoomed=True" }
else { $results["dblclick-maximize"] = "FAIL IsZoomed=False rect=$($r1.Left),$($r1.Top),$($r1.Right),$($r1.Bottom)" }

# 4) double-click again -> restore original size
Invoke-DoubleClickAt (Get-DragX $hwnd) (Get-BtnY $hwnd)
$r2 = Get-Rect $hwnd
$r0w = $r0.Right - $r0.Left; $r0h = $r0.Bottom - $r0.Top
$r2w = $r2.Right - $r2.Left; $r2h = $r2.Bottom - $r2.Top
$restoreOk = (-not [WinVerify]::IsZoomed($hwnd)) `
  -and ([Math]::Abs($r2w - $r0w) -le 12) -and ([Math]::Abs($r2h - $r0h) -le 12)
if ($restoreOk) { $results["dblclick-restore"] = "PASS size=${r2w}x${r2h} vs ${r0w}x${r0h}" }
else { $results["dblclick-restore"] = "FAIL IsZoomed=$([WinVerify]::IsZoomed($hwnd)) size=${r2w}x${r2h} vs ${r0w}x${r0h}" }

# 5) close
Invoke-ClickAt (Get-CloseX $hwnd) (Get-BtnY $hwnd)
Start-Sleep -Milliseconds 2500
$still = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
if ($null -eq $still) { $results["close"] = "PASS pid=$($proc.Id) exited" }
else { $results["close"] = "FAIL pid=$($proc.Id) still running" }

$results.GetEnumerator() | ForEach-Object { Write-Output ("{0}: {1}" -f $_.Key, $_.Value) }
