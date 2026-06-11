; RIGStats NSIS installer
; Produces a per-machine installer that:
;   - Installs rigstats-egui.exe + rigstats-sensor.exe + PawnIO driver
;   - Registers the sensor service (LocalSystem, auto-start)
;   - Creates Start Menu shortcut and uninstaller
;
; Build command (from repo root, after cargo build --release):
;   makensis /DVERSION=1.25.0 build\installer.nsi
;
; Required files relative to repo root:
;   target\release\rigstats-egui.exe
;   sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\rigstats-sensor.exe
;   build\pawnio\PawnIO.sys
;   build\pawnio\pawnio.inf
;   build\pawnio\PawnIO.cat
;   assets\icon.ico
;   CHANGELOG.md

Unicode True
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

Name "RIGStats ${VERSION}"
OutFile "target\release\RIGStats_${VERSION}_x64-setup.exe"
InstallDir "$PROGRAMFILES64\RIGStats"
InstallDirRegKey HKLM "Software\RIGStats" "InstallDir"
RequestExecutionLevel admin

!include "MUI2.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "assets\icon.ico"
!define MUI_UNICON "assets\icon.ico"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; ── Pre-install ────────────────────────────────────────────────────────────────
Function .onInit
  ; Stop sensor service before overwriting files.
  nsExec::ExecToLog 'cmd /C sc stop rigstats-sensor >NUL 2>&1'
  Sleep 3000
  ; Kill old LHM artefacts from pre-sidecar versions (< 1.20).
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "RIGStats\LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "RigStats\LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C taskkill /F /IM LibreHardwareMonitor.exe >NUL 2>&1'
  Sleep 1000
FunctionEnd

; ── Main section ───────────────────────────────────────────────────────────────
Section "RIGStats" SecMain
  SectionIn RO

  SetOutPath "$INSTDIR"
  File "target\release\rigstats-egui.exe"
  File "sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\rigstats-sensor.exe"
  File "CHANGELOG.md"

  SetOutPath "$INSTDIR\pawnio"
  File "build\pawnio\PawnIO.sys"
  File "build\pawnio\pawnio.inf"
  File "build\pawnio\PawnIO.cat"

  ; ── PawnIO kernel driver ──────────────────────────────────────────────────
  nsExec::ExecToStack '"$WINDIR\Sysnative\pnputil.exe" /add-driver "$INSTDIR\pawnio\pawnio.inf" /install'
  Pop $R0
  Pop $R1
  DetailPrint "PawnIO install: exit $R0 — $R1"

  ; ── Remove old service entry, re-create with fresh binary path ────────────
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'
  Sleep 1000
  nsExec::ExecToStack 'cmd /C sc create rigstats-sensor binPath= "$INSTDIR\rigstats-sensor.exe" start= auto obj= LocalSystem displayname= "RIGStats Sensor"'
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
    "DisplayIcon"      "$INSTDIR\rigstats-egui.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "Publisher"        "codeby.se"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "NoModify"         1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "NoRepair"         1

  CreateDirectory "$SMPROGRAMS\RIGStats"
  CreateShortcut "$SMPROGRAMS\RIGStats\RIGStats.lnk" "$INSTDIR\rigstats-egui.exe"
  CreateShortcut "$SMPROGRAMS\RIGStats\Uninstall RIGStats.lnk" "$INSTDIR\uninstall.exe"

  WriteUninstaller "$INSTDIR\uninstall.exe"
SectionEnd

; ── Uninstaller ────────────────────────────────────────────────────────────────
Section "Uninstall"
  nsExec::ExecToLog 'cmd /C sc stop rigstats-sensor >NUL 2>&1'
  Sleep 2000
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'

  ; Kill the main app if running.
  nsExec::ExecToLog 'cmd /C taskkill /F /IM rigstats-egui.exe >NUL 2>&1'
  Sleep 1000

  Delete "$INSTDIR\rigstats-egui.exe"
  Delete "$INSTDIR\rigstats-sensor.exe"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\pawnio"
  RMDir "$INSTDIR"

  Delete "$SMPROGRAMS\RIGStats\RIGStats.lnk"
  Delete "$SMPROGRAMS\RIGStats\Uninstall RIGStats.lnk"
  RMDir  "$SMPROGRAMS\RIGStats"

  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats"
  DeleteRegKey HKLM "Software\RIGStats"

  ; Remove old LHM tasks if still present.
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
SectionEnd
