; RIGStats NSIS installer
; Produces a per-machine installer that:
;   - Installs rigstats.exe + rigstats-wallpaper.exe + rigstats-sensor.exe + PawnIO driver
;   - Registers the sensor service (LocalSystem, auto-start)
;   - Creates Start Menu shortcut and uninstaller
;
; Build command (from repo root, after cargo build --release):
;   makensis /DVERSION=1.25.0 build\installer.nsi
;
; Required files relative to repo root:
;   target\release\rigstats.exe
;   target\release\rigstats-wallpaper.exe
;   sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\rigstats-sensor.exe
;   build\pawnio\PawnIO_setup.exe  (official setup, pinned by PAWNIO_SETUP_SHA256)
;   assets\icon.ico
;   CHANGELOG.md

Unicode True
SetCompressor /SOLID lzma

; Change working directory to repo root so all paths are relative to it.
!cd ..

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

; PawnIO 2.2.0 — https://github.com/namazso/PawnIO.Setup/releases/tag/2.2.0
; Fail the build if the bundled setup is not exactly that release.
!define PAWNIO_SETUP_SHA256 "1F519A22E47187F70A1379A48CA604981C4FCF694F4E65B734AAA74A9FBA3032"
!system 'certutil -hashfile build\pawnio\PawnIO_setup.exe SHA256 | findstr /I /X "${PAWNIO_SETUP_SHA256}"' = 0

Name "RIGStats ${VERSION}"
OutFile "target\release\RIGStats_${VERSION}_x64-setup.exe"
InstallDir "$PROGRAMFILES64\RIGStats"
InstallDirRegKey HKLM "Software\RIGStats" "InstallDir"
RequestExecutionLevel admin

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

Var /GLOBAL AutoUpdate

!define MUI_ICON "assets\icon.ico"
!define MUI_UNICON "assets\icon.ico"
!define MUI_ABORTWARNING

!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Launch RIGStats"
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchRIGStats

!define MUI_PAGE_CUSTOMFUNCTION_PRE ComponentsPre
!insertmacro MUI_PAGE_COMPONENTS
!define MUI_PAGE_CUSTOMFUNCTION_PRE DirectoryPre
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_PAGE_CUSTOMFUNCTION_PRE FinishPre
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; Launch RIGStats without the installer's elevated token so the app
; runs with normal user privileges (expected for a tray application).
Function LaunchRIGStats
  Exec '"$INSTDIR\rigstats.exe"'
FunctionEnd

; ── Pre-install ────────────────────────────────────────────────────────────────
Function .onInit
  ; Detect /autoupdate flag: show progress window but skip wizard pages.
  ${GetOptions} $CMDLINE "/autoupdate" $R0
  ${IfNot} ${Errors}
    StrCpy $AutoUpdate 1
    SetAutoClose true
  ${EndIf}

  ; Stop the running app and sensor service before overwriting files.
  nsExec::ExecToLog 'cmd /C taskkill /F /IM rigstats.exe >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C sc stop rigstats-sensor >NUL 2>&1'
  Sleep 3000
  ; Kill old LHM artefacts from pre-sidecar versions (< 1.20).
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "RIGStats\LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "RigStats\LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C taskkill /F /IM LibreHardwareMonitor.exe >NUL 2>&1'
  Sleep 1000
FunctionEnd

; ── Page skip functions (used when /autoupdate is passed) ──────────────────────
Function ComponentsPre
  ${If} $AutoUpdate == 1
    Abort
  ${EndIf}
FunctionEnd

Function DirectoryPre
  ${If} $AutoUpdate == 1
    Abort
  ${EndIf}
FunctionEnd

Function FinishPre
  ${If} $AutoUpdate == 1
    Abort
  ${EndIf}
FunctionEnd

; ── Main section ───────────────────────────────────────────────────────────────
Section "RIGStats" SecMain
  SectionIn RO

  SetOutPath "$INSTDIR"
  ; Remove old Tauri binary name if upgrading from pre-egui version.
  Delete "$INSTDIR\rigstats.exe"
  File "target\release\rigstats.exe"
  File "target\release\rigstats-wallpaper.exe"
  File "sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\rigstats-sensor.exe"
  ; Native libs that may be emitted alongside the single-file exe depending on
  ; the .NET runtime pack. They are Unix P/Invoke helpers never loaded on
  ; Windows, so /nonfatal keeps the build working whether or not they appear.
  File /nonfatal "sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\MonoPosixHelper.dll"
  File /nonfatal "sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\libMonoPosixHelper.dll"
  File "CHANGELOG.md"

  ; Driver files staged here by versions before the PawnIO setup was bundled.
  RMDir /r "$INSTDIR\pawnio"

  ; ── PawnIO kernel driver ──────────────────────────────────────────────────
  ; The official setup creates the Root\PawnIO device node and the PawnIO
  ; service; `pnputil /add-driver` only staged the driver, so a machine without
  ; another PawnIO consumer never got the service (#205). It also upgrades or
  ; keeps an existing install shared with other tools. Run from the temp
  ; plugins dir — nothing is left in $INSTDIR.
  ; Exit 183 (ERROR_ALREADY_EXISTS) = same version already installed; fine.
  ; The `sc query PawnIO` result below is the real success signal.
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File "build\pawnio\PawnIO_setup.exe"
  nsExec::ExecToStack '"$PLUGINSDIR\PawnIO_setup.exe" -install -silent'
  Pop $R0
  Pop $R1
  DetailPrint "PawnIO install: exit $R0"
  nsExec::ExecToStack '"$SYSDIR\sc.exe" query PawnIO'
  Pop $R2
  Pop $R3
  DetailPrint "PawnIO service query: exit $R2"
  SetOutPath "$INSTDIR"

  ; ── Crash dumps for the sensor service (#220) ──────────────────────────────
  ; A native crash (inside a driver call) can't be caught in .NET; Windows
  ; Error Reporting keeps a minidump of it, the newest 3, in a folder the
  ; service locks to administrators (DataDirectory.EnsureDumpFolder) before
  ; Windows writes into it. 64-bit view: WER doesn't read WOW6432Node for a
  ; 64-bit process.
  SetRegView 64
  WriteRegExpandStr HKLM "SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\rigstats-sensor.exe" \
    "DumpFolder" "%ProgramData%\se.codeby.rigstats\dumps"
  WriteRegDWORD HKLM "SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\rigstats-sensor.exe" \
    "DumpType" 1
  WriteRegDWORD HKLM "SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\rigstats-sensor.exe" \
    "DumpCount" 3
  SetRegView default

  ; ── Remove old service entry, re-create with fresh binary path ────────────
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'
  Sleep 1000
  ; The path is stored quoted (\" inside the argument): unquoted, a path with
  ; spaces lets Windows try e.g. C:\Program.exe first.
  nsExec::ExecToStack 'cmd /C sc create rigstats-sensor binPath= "\"$INSTDIR\rigstats-sensor.exe\"" start= auto obj= LocalSystem displayname= "RIGStats Sensor"'
  Pop $4
  Pop $5
  DetailPrint "Service create: exit $4"
  nsExec::ExecToLog 'cmd /C sc description rigstats-sensor "Reads hardware sensors for the RIGStats dashboard." >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C sc failure rigstats-sensor reset= 60 actions= restart/5000/restart/10000/restart/30000 >NUL 2>&1'
  nsExec::ExecToStack 'cmd /C sc start rigstats-sensor >NUL 2>&1'
  Pop $6
  Pop $7
  DetailPrint "Service start: exit $6"

  ; ── Remove old LHM tasks / dirs from pre-sidecar versions ─────────────────
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RIGStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
  RMDir /r "$INSTDIR\lhm"

  ; ── Registry & shortcuts ─────────────────────────────────────────────────
  WriteRegStr HKLM "Software\RIGStats" "InstallDir" "$INSTDIR"
  WriteRegStr HKLM "Software\RIGStats" "Version"    "${VERSION}"

  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "DisplayName"      "RIGStats"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "DisplayVersion"   "${VERSION}"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "UninstallString"  "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "DisplayIcon"      "$INSTDIR\rigstats.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "Publisher"        "codeby.se"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "NoModify"         1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "NoRepair"         1

  CreateDirectory "$SMPROGRAMS\RIGStats"
  CreateShortcut "$SMPROGRAMS\RIGStats\RIGStats.lnk" "$INSTDIR\rigstats.exe"
  CreateShortcut "$SMPROGRAMS\RIGStats\Uninstall RIGStats.lnk" "$INSTDIR\uninstall.exe"

  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; ── Write install log to ProgramData for diagnostics ─────────────────────
  ReadEnvStr $8 PROGRAMDATA
  CreateDirectory "$8\se.codeby.rigstats"
  FileOpen $9 "$8\se.codeby.rigstats\rigstats-install.log" w
  FileWrite $9 "version=${VERSION}$\r$\n"
  FileWrite $9 "install_dir=$INSTDIR$\r$\n"
  FileWrite $9 "pawnio_exit=$R0$\r$\n"
  FileWrite $9 "pawnio_output=$R1$\r$\n"
  FileWrite $9 "pawnio_service_query_exit=$R2$\r$\n"
  FileWrite $9 "pawnio_service_query=$R3$\r$\n"
  FileWrite $9 "service_create_exit=$4$\r$\n"
  FileWrite $9 "service_start_exit=$6$\r$\n"
  FileClose $9

  ; ── Auto-launch with update notification (in-app /autoupdate installs only) ──
  ; Manual installs use the finish-page "Launch RIGStats" checkbox instead.
  ${If} $AutoUpdate == 1
    Exec '"$INSTDIR\rigstats.exe" "--just-updated=${VERSION}"'
  ${EndIf}
SectionEnd

; ── Optional: desktop shortcut ────────────────────────────────────────────────
Section /o "Desktop Shortcut" SecDesktop
  CreateShortcut "$DESKTOP\RIGStats.lnk" "$INSTDIR\rigstats.exe"
SectionEnd

; ── Uninstaller ────────────────────────────────────────────────────────────────
Section "Uninstall"
  nsExec::ExecToLog 'cmd /C sc stop rigstats-sensor >NUL 2>&1'
  Sleep 2000
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'

  ; Kill the main app if running.
  nsExec::ExecToLog 'cmd /C taskkill /F /IM rigstats.exe >NUL 2>&1'
  Sleep 1000

  Delete "$INSTDIR\rigstats.exe"
  Delete "$INSTDIR\rigstats-wallpaper.exe"
  Delete "$INSTDIR\rigstats-sensor.exe"
  Delete "$INSTDIR\MonoPosixHelper.dll"
  Delete "$INSTDIR\libMonoPosixHelper.dll"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\pawnio"
  RMDir "$INSTDIR"

  Delete "$DESKTOP\RIGStats.lnk"
  Delete "$SMPROGRAMS\RIGStats\RIGStats.lnk"
  Delete "$SMPROGRAMS\RIGStats\Uninstall RIGStats.lnk"
  RMDir  "$SMPROGRAMS\RIGStats"

  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats"
  DeleteRegKey HKLM "Software\RIGStats"

  ; Crash dumps (#220): the WER setting and the dumps themselves.
  SetRegView 64
  DeleteRegKey HKLM "SOFTWARE\Microsoft\Windows\Windows Error Reporting\LocalDumps\rigstats-sensor.exe"
  SetRegView default
  ReadEnvStr $8 PROGRAMDATA
  RMDir /r "$8\se.codeby.rigstats\dumps"

  ; Remove old LHM tasks if still present.
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
SectionEnd
