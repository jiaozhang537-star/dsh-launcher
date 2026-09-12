# Phase 0 visual acceptance: capture the app main window to a PNG.
# Usage (repo root): & .\scripts\capture-window.ps1 -ProcessName "dsh-launcher" -OutPath <png path>
# NOTE: keep this file ASCII-only; it is read by Windows PowerShell without a BOM.
param(
  [Parameter(Mandatory = $true)][string]$ProcessName,
  [Parameter(Mandatory = $true)][string]$OutPath
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

Add-Type @"
using System;
using System.Runtime.InteropServices;

public static class WinCap {
  public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
  [StructLayout(LayoutKind.Sequential)]
  public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);

  // Largest visible top-level window of the process above a sane minimum size.
  public static IntPtr FindMainByPid(uint targetPid) {
    IntPtr best = IntPtr.Zero;
    long bestArea = -1;
    EnumWindows((hWnd, lParam) => {
      uint pid; GetWindowThreadProcessId(hWnd, out pid);
      if (pid == targetPid && IsWindowVisible(hWnd)) {
        RECT r; GetWindowRect(hWnd, out r);
        int w = r.Right - r.Left;
        int h = r.Bottom - r.Top;
        if (w < 300 || h < 200) { return true; }  // skip splash/placeholder windows
        long area = (long)w * h;
        if (area > bestArea) { bestArea = area; best = hWnd; }
      }
      return true;
    }, IntPtr.Zero);
    return best;
  }
}
"@

[void][WinCap]::SetProcessDPIAware()

$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { throw "process not found: $ProcessName" }

$hwnd = [WinCap]::FindMainByPid([uint32]$proc.Id)
if ($hwnd -eq [IntPtr]::Zero) {
  Start-Sleep -Milliseconds 2500
  $hwnd = [WinCap]::FindMainByPid([uint32]$proc.Id)
}
if ($hwnd -eq [IntPtr]::Zero) { throw "no visible top-level window for pid $($proc.Id)" }

[void][WinCap]::ShowWindow($hwnd, 9)   # SW_RESTORE
[void][WinCap]::SetForegroundWindow($hwnd)
Start-Sleep -Milliseconds 500

$rect = New-Object WinCap+RECT
[void][WinCap]::GetWindowRect($hwnd, [ref]$rect)
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
if ($width -le 0 -or $height -le 0) { throw "bad window rect: ${width}x${height}" }

$bitmap = New-Object System.Drawing.Bitmap($width, $height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, (New-Object System.Drawing.Size($width, $height)))
$graphics.Dispose()

$outDir = Split-Path -Parent $OutPath
if ($outDir -and -not (Test-Path $outDir)) { New-Item -ItemType Directory -Path $outDir -Force | Out-Null }
$bitmap.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bitmap.Dispose()

Write-Output "saved=$OutPath size=${width}x${height}"
