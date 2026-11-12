# Dev tool: opens the app on a books file, screenshots its window, optionally clicks (x,y offsets in the shot) and screenshots again.
# Needs a real desktop session (run outside any sandbox). Usage:
#   pwsh -File tools/winshot.ps1 -Exe target/debug/AccountingCraft.exe -Db sample.db -Out <dir> -Clicks "77,173;t:hello{TAB};97,215"
# A step starting with t: types its text (SendKeys codes like {TAB} and {ENTER} work); w:-10 scrolls down 10 notches.
param([string]$Exe, [string]$Db, [string]$Out, [string]$Clicks = "")
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool r);
  public struct RECT { public int L, T, R, B; }
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  public static IntPtr Find(uint pid) {
    IntPtr found = IntPtr.Zero;
    EnumWindows((h, l) => { uint p; GetWindowThreadProcessId(h, out p); RECT r;
      if (p == pid && IsWindowVisible(h) && GetWindowRect(h, out r) && r.R - r.L > 300) { found = h; return false; } return true; }, IntPtr.Zero);
    return found;
  }
}
"@
[W]::SetProcessDPIAware() | Out-Null
$p = Start-Process -FilePath $Exe -ArgumentList "`"$Db`"" -PassThru
$h = [IntPtr]::Zero
for ($t = 0; $t -lt 80 -and $h -eq [IntPtr]::Zero; $t++) { Start-Sleep -Milliseconds 250; $h = [W]::Find([uint32]$p.Id) }
[W]::MoveWindow($h, 60, 60, 1600, 1000, $true) | Out-Null
[W]::SetForegroundWindow($h) | Out-Null
Start-Sleep 2
function Shot($name) {
  [W+RECT]$r = New-Object W+RECT; [W]::GetWindowRect($h, [ref]$r) | Out-Null
  $bmp = New-Object System.Drawing.Bitmap ($r.R - $r.L), ($r.B - $r.T)
  $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
  $bmp.Save("$Out\$name.png"); $g.Dispose(); $bmp.Dispose()
  return $r
}
$r = Shot "shot0"
$i = 1
foreach ($c in ($Clicks -split ";" | Where-Object { $_ })) {
  if ($c.StartsWith("t:")) {
    [System.Windows.Forms.SendKeys]::SendWait($c.Substring(2)); Start-Sleep -Milliseconds 900
    Shot "shot$i" | Out-Null; $i++; continue
  }
  if ($c.StartsWith("w:")) {
    [W]::SetCursorPos($r.L + 900, $r.T + 500) | Out-Null
    [W]::mouse_event(0x0800, 0, 0, 120 * [int]$c.Substring(2), 0); Start-Sleep -Milliseconds 900
    Shot "shot$i" | Out-Null; $i++; continue
  }
  $xy = $c -split ","
  [W]::SetCursorPos($r.L + [int]$xy[0], $r.T + [int]$xy[1]) | Out-Null
  Start-Sleep -Milliseconds 250; [W]::mouse_event(2, 0, 0, 0, 0); Start-Sleep -Milliseconds 80; [W]::mouse_event(4, 0, 0, 0, 0)
  Start-Sleep -Milliseconds 900
  Shot "shot$i" | Out-Null; $i++
}
Stop-Process -Id $p.Id -Force
"done $($r.L),$($r.T),$($r.R),$($r.B)"
