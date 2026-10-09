---
name: verifier-gui
description: Verify GUI changes in RigStats by building, launching, and screenshotting the running app. Use for any /verify task involving the egui UI, panels, settings windows, or dialog boxes.
---

RigStats is a Windows-native egui app. There is no DOM or Playwright — observation is via PowerShell screenshots. See `run-rigstats.md` for build/launch/monitor details.

## Verification workflow

### 1. Build and launch

```powershell
# Kill any running instance (by PID — name-based kill silently fails)
$proc = Get-Process rigstats -ErrorAction SilentlyContinue
if ($proc) { Stop-Process -Id $proc.Id -Force }

cargo build --manifest-path src-egui/Cargo.toml

# Verify timestamp advanced before launching
(Get-Item .\target\debug\rigstats.exe).LastWriteTime
Start-Process .\target\debug\rigstats.exe
Start-Sleep 3
```

### 2. Screenshot the portrait dashboard

```powershell
Add-Type -AssemblyName System.Windows.Forms,System.Drawing

# Full dashboard
$bmp = New-Object System.Drawing.Bitmap(450, 1920)
$g   = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen([System.Drawing.Point]::new(2560, 0), [System.Drawing.Point]::Empty, [System.Drawing.Size]::new(450, 1920))
$bmp.Save("$env:TEMP\rigstats_full.png"); $g.Dispose(); $bmp.Dispose()

# Header panel only (top 300 px)
$bmp = New-Object System.Drawing.Bitmap(450, 300)
$g   = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen([System.Drawing.Point]::new(2560, 0), [System.Drawing.Point]::Empty, [System.Drawing.Size]::new(450, 300))
$bmp.Save("$env:TEMP\rigstats_header.png"); $g.Dispose(); $bmp.Dispose()
```

Then `Read` the saved PNG to view it.

### 3. Open the Settings window

The tray icon lives in the system notification area. Right-click it to get the context menu → Settings. Alternatively, interact via Windows automation:

```powershell
# Option A: send a WM_RBUTTONUP to the tray area (fragile, position-dependent)
# Option B: use App Control keys if available
# Option C: for verification purposes, check if Settings is already open:
Get-Process rigstats | Select-Object MainWindowTitle
# Returns "RigStats — Settings" when Settings viewport is open
```

The most reliable approach is to open Settings manually before invoking the verifier, or ask the user to open it.

### 4. Screenshot a specific dialog window

```powershell
Add-Type @"
using System; using System.Runtime.InteropServices;
public class WinApi {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    public struct RECT { public int Left, Top, Right, Bottom; }
}
"@

$proc = Get-Process rigstats
$hwnd = [IntPtr]$proc.MainWindowHandle
[WinApi]::SetForegroundWindow($hwnd) | Out-Null
$r = New-Object WinApi+RECT
[WinApi]::GetWindowRect($hwnd, [ref]$r) | Out-Null
$w = $r.Right - $r.Left; $h = $r.Bottom - $r.Top
Write-Host "Window at ($($r.Left),$($r.Top)) size ${w}x${h}"

Add-Type -AssemblyName System.Drawing
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$g   = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen([System.Drawing.Point]::new($r.Left, $r.Top), [System.Drawing.Point]::Empty, [System.Drawing.Size]::new($w, $h))
$bmp.Save("$env:TEMP\rigstats_dialog.png"); $g.Dispose(); $bmp.Dispose()
```

### 5. Simulate keyboard/mouse input (for interacting with dialogs)

```powershell
Add-Type -AssemblyName System.Windows.Forms

# Click at absolute screen coordinates
[System.Windows.Forms.Cursor]::Position = [System.Drawing.Point]::new($x, $y)
Add-Type @"
using System; using System.Runtime.InteropServices;
public class Mouse {
    [DllImport("user32.dll")] public static extern void mouse_event(int f, int x, int y, int d, int e);
    public static void Click(int x, int y) { mouse_event(0x8000|0x02, x, y, 0, 0); mouse_event(0x8000|0x04, x, y, 0, 0); }
}
"@
[Mouse]::Click($x, $y)

# Type text into a focused field
[System.Windows.Forms.SendKeys]::SendWait("My Custom Text")

# Press Tab or Enter
[System.Windows.Forms.SendKeys]::SendWait("{TAB}")
[System.Windows.Forms.SendKeys]::SendWait("{ENTER}")

# Select all + delete (clear a text field)
[System.Windows.Forms.SendKeys]::SendWait("^a{DELETE}")
```

## What lives where on screen

| Panel | Portrait Y range (approx) |
|---|---|
| Header (hostname + model_name + brand) | 0–120 px |
| Clock | 120–220 px |
| CPU load | 220–430 px |
| GPU load | 430–640 px |
| RAM usage | 640–800 px |
| Network | 800–980 px |
| Disk | 980–1120 px |

Settings / About / Updater dialogs open on the **primary monitor** (DISPLAY1, X=0).

## Common verification scenarios

**Check a label or value changed in the dashboard:**
→ Screenshot `$env:TEMP\rigstats_header.png` or the relevant Y slice of the portrait monitor. Read the file to view it.

**Check a settings dialog label/field:**
→ Open Settings, screenshot the dialog window using the `GetWindowRect` snippet above.

**Check live preview works (Settings change reflects in dashboard immediately):**
→ Screenshot the portal header before and after typing in the Settings field.

**Check a value after Save + restart:**
→ Save in Settings, kill, rebuild, relaunch, screenshot.

## Gotchas

- **Portrait monitor is at X=2560** — not the primary screen. A primary-screen screenshot will not contain the dashboard.
- **`Stop-Process -Name`** silently fails on a locked exe. Always kill by PID.
- **`MainWindowHandle` changes** each time a secondary viewport (Settings, About) opens/closes — re-query it each use.
- **`SendKeys` needs focus** — always call `SetForegroundWindow` on the target window first.
- **egui redraws continuously** — no need to wait for "render complete"; a 500 ms sleep after an action is enough.
