# Guided, read-only live check of an ASUS keyboard's profile event and its
# 12 00 / 12 01 replies (#457, docs/keyboard-research.md). Opens a console
# that tells the owner what to do, logs every input report of the keyboard's
# collections (all but the mi_00 keyboard/pointer collections) and writes the
# prompts and reports to -Log.
#
# Sent to the keyboard: only `12 00` and `12 01` (documented by Gear Link and
# G-Helper; 12 01 is the battery question RIGStats already asks), at the start
# and at the end. Nothing else is written.
#
#   pwsh -File tools\keyboard-listen-guided.ps1                    # Azoth X on the Omni receiver
#   pwsh -File tools\keyboard-listen-guided.ps1 -ProductId 1c24 -Report 0   # by cable
param(
    [string]$ProductId = "1ace",
    [byte]$Report = 2,
    [string]$Log = "$env:TEMP\keyboard-listen.log"
)
$code = @'
using System; using System.Runtime.InteropServices; using System.Threading; using System.Collections.Generic; using System.Text;
public static class KbListen {
    [StructLayout(LayoutKind.Sequential)] public struct SP_DEVICE_INTERFACE_DATA { public int cbSize; public Guid g; public int flags; public IntPtr r; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] public struct SP_DEVICE_INTERFACE_DETAIL_DATA { public int cbSize; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 512)] public string path; }
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] static extern IntPtr SetupDiGetClassDevs(ref Guid g, IntPtr e, IntPtr h, int flags);
    [DllImport("setupapi.dll")] static extern bool SetupDiEnumDeviceInterfaces(IntPtr h, IntPtr d, ref Guid g, int i, ref SP_DEVICE_INTERFACE_DATA data);
    [DllImport("setupapi.dll", CharSet = CharSet.Unicode)] static extern bool SetupDiGetDeviceInterfaceDetail(IntPtr h, ref SP_DEVICE_INTERFACE_DATA d, ref SP_DEVICE_INTERFACE_DETAIL_DATA detail, int size, out int req, IntPtr di);
    [DllImport("setupapi.dll")] static extern bool SetupDiDestroyDeviceInfoList(IntPtr h);
    [DllImport("hid.dll")] static extern void HidD_GetHidGuid(out Guid g);
    [DllImport("hid.dll")] static extern bool HidD_GetPreparsedData(IntPtr h, out IntPtr pp);
    [DllImport("hid.dll")] static extern bool HidD_FreePreparsedData(IntPtr pp);
    [StructLayout(LayoutKind.Sequential)] public struct HIDP_CAPS { public ushort Usage, UsagePage, InputReportByteLength, OutputReportByteLength, FeatureReportByteLength; [MarshalAs(UnmanagedType.ByValArray, SizeConst = 17)] public ushort[] Reserved; public ushort a, b, c, d, e, f, g, h, i, j; }
    [DllImport("hid.dll")] static extern int HidP_GetCaps(IntPtr pp, out HIDP_CAPS caps);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)] static extern IntPtr CreateFile(string p, uint a, uint s, IntPtr sec, uint c, uint f, IntPtr t);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool ReadFile(IntPtr h, byte[] buf, int n, out int read, IntPtr ov);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool WriteFile(IntPtr h, byte[] buf, int n, out int written, IntPtr ov);
    [DllImport("kernel32.dll")] static extern bool CancelIoEx(IntPtr h, IntPtr ov);
    public static List<string> Lines = new List<string>();
    static readonly object Lock = new object();
    public static void Log(string s) { lock (Lock) { Lines.Add(DateTime.Now.ToString("HH:mm:ss.fff") + " " + s); } }
    static readonly List<IntPtr> handles = new List<IntPtr>();
    static string commandPath; static int commandOut;

    // Opens every collection of the device except mi_00 for reading (shared),
    // and remembers the vendor page 0xFF00 collection with output reports as
    // the command channel.
    public static int Start(string pid) {
        Guid g; HidD_GetHidGuid(out g);
        IntPtr set = SetupDiGetClassDevs(ref g, IntPtr.Zero, IntPtr.Zero, 0x12);
        var data = new SP_DEVICE_INTERFACE_DATA(); data.cbSize = Marshal.SizeOf(data);
        string match = "vid_0b05&pid_" + pid.ToLowerInvariant();
        for (int i = 0; SetupDiEnumDeviceInterfaces(set, IntPtr.Zero, ref g, i, ref data); i++) {
            var detail = new SP_DEVICE_INTERFACE_DETAIL_DATA(); detail.cbSize = IntPtr.Size == 8 ? 8 : 6; int req;
            if (!SetupDiGetDeviceInterfaceDetail(set, ref data, ref detail, Marshal.SizeOf(detail), out req, IntPtr.Zero)) continue;
            string path = detail.path;
            if (path.IndexOf(match, StringComparison.OrdinalIgnoreCase) < 0 || path.IndexOf("&mi_00", StringComparison.OrdinalIgnoreCase) >= 0) continue;
            IntPtr q = CreateFile(path, 0, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
            if (q == new IntPtr(-1)) continue;
            HIDP_CAPS c = new HIDP_CAPS(); IntPtr pp;
            if (HidD_GetPreparsedData(q, out pp)) { HidP_GetCaps(pp, out c); HidD_FreePreparsedData(pp); }
            CloseHandle(q);
            if (c.InputReportByteLength < 2) continue;
            string label = path.Substring(path.IndexOf("&mi_", StringComparison.OrdinalIgnoreCase) + 1).Split('#')[0] + " page " + c.UsagePage.ToString("X4");
            if (c.UsagePage == 0xFF00 && c.OutputReportByteLength > 0) { commandPath = path; commandOut = c.OutputReportByteLength; }
            IntPtr h = CreateFile(path, 0x80000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
            if (h == new IntPtr(-1)) { Log(label + ": open failed " + Marshal.GetLastWin32Error()); continue; }
            handles.Add(h); int len = c.InputReportByteLength; Log("listening: " + label);
            var t = new Thread(() => { var buf = new byte[len]; while (true) { int n; if (!ReadFile(h, buf, buf.Length, out n, IntPtr.Zero)) break; Log(label + " IN " + BitConverter.ToString(buf, 0, n)); } });
            t.IsBackground = true; t.Start();
        }
        SetupDiDestroyDeviceInfoList(set);
        return handles.Count;
    }

    // Sends one documented question ([report, command, key]) on the command
    // channel; its reply arrives through the listener.
    public static void Ask(byte report, byte command, byte key) {
        if (commandPath == null) { Log("no command channel found"); return; }
        IntPtr h = CreateFile(commandPath, 0xC0000000, 3, IntPtr.Zero, 3, 0, IntPtr.Zero);
        if (h == new IntPtr(-1)) { Log("command channel open failed " + Marshal.GetLastWin32Error()); return; }
        var buf = new byte[commandOut]; buf[0] = report; buf[1] = command; buf[2] = key; int w;
        bool ok = WriteFile(h, buf, buf.Length, out w, IntPtr.Zero);
        Log(string.Format("SENT {0:X2} {1:X2} {2:X2} ({3})", report, command, key, ok ? "ok" : "failed " + Marshal.GetLastWin32Error()));
        CloseHandle(h);
    }

    public static void Stop() { foreach (var h in handles) { CancelIoEx(h, IntPtr.Zero); CloseHandle(h); } }
}
'@
Add-Type -TypeDefinition $code
$host.UI.RawUI.WindowTitle = "RIGStats - test av tangentbordet"
function Say($text, $color) { Write-Host ""; Write-Host $text -ForegroundColor $color; [KbListen]::Log("PROMPT: $text") }
# Waits for Enter and logs what was typed (a profile key that also types a
# character shows up here, so the log tells it).
function Step($text) {
    Say $text Yellow
    $typed = Read-Host "Tryck Enter när du är klar"
    [KbListen]::Log("ENTER (typed: '$typed')")
}
function Questions($when) {
    [KbListen]::Log("--- questions ($when) ---")
    [KbListen]::Ask($Report, 0x12, 0x00); Start-Sleep -Milliseconds 400
    [KbListen]::Ask($Report, 0x12, 0x01); Start-Sleep -Milliseconds 400
}

Say "RIGStats - test av tangentbordets profiler (bara läsning)" Cyan
Write-Host "Det här fönstret lyssnar på tangentbordet och frågar det två saker (vilken profil och batteri)."
Write-Host "Inget ändras i tangentbordet av testet. Gör INGET annat än det som står i de gula rutorna,"
Write-Host "och klicka inte i andra fönster - det här fönstret ska vara aktivt hela tiden."
$n = [KbListen]::Start($ProductId)
[KbListen]::Log("listening on $n collections of 0B05:$ProductId, report 0x$('{0:X2}' -f $Report)")
Step "STEG 1 av 5: Rör inte tangentbordet. Tryck bara Enter för att börja."
Questions "start"
Step "STEG 2 av 5: Håll ned Fn-tangenten, tryck siffran 2 (sifferraden ovanför bokstäverna, inte numpad), släpp båda. Vänta 2 sekunder."
Step "STEG 3 av 5: Håll ned Fn-tangenten, tryck siffran 3, släpp båda. Vänta 2 sekunder."
Step "STEG 4 av 5: Håll ned Fn-tangenten, tryck siffran 1, släpp båda. (Tillbaka till profil 1.) Vänta 2 sekunder."
Questions "end"
Start-Sleep -Milliseconds 600
Say "STEG 5 av 5: Ändrades något på tangentbordet när du tryckte Fn + 2 och Fn + 3 (belysning, OLED-skärmen)?" Yellow
$seen = Read-Host "Skriv vad du såg (eller 'inget') och tryck Enter"
[KbListen]::Log("OWNER SAW: $seen")
[KbListen]::Stop()
[KbListen]::Lines | Set-Content -Path $Log -Encoding UTF8
Say "Klart - tack! Loggen är sparad i $Log. Du kan stänga fönstret." Green
Read-Host "Tryck Enter för att stänga"
