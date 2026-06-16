; RIGStats NSIS installer
; Produces a per-machine installer that:
;   - Installs rigstats.exe + rigstats-sensor.exe + PawnIO driver
;   - Registers the sensor service (LocalSystem, auto-start)
;   - Creates All-Users Start Menu shortcut and uninstaller
;   - Uses the UAC plugin (administrator broker model) so the app is installed
;     and launched for the real interactive user, not the admin who approved the
;     UAC prompt (fixes over-the-shoulder elevation binding to the wrong account)
;
; Build command (from repo root, after cargo build --release):
;   makensis /DVERSION=1.25.0 build\installer.nsi
; The vendored UAC plugin under build\nsis-plugins is referenced via
; !addplugindir, so no NSIS plugin-dir setup is required (local or CI).
;
; Required files relative to repo root:
;   target\release\rigstats.exe
;   sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\rigstats-sensor.exe
;   build\pawnio\PawnIO.sys
;   build\pawnio\pawnio.inf
;   build\pawnio\PawnIO.cat
;   assets\icon.ico
;   CHANGELOG.md

Unicode True
SetCompressor /SOLID lzma

; Change working directory to repo root so all paths are relative to it.
!cd ..

!ifndef VERSION
  !define VERSION "0.0.0"
!endif

Name "RIGStats ${VERSION}"
OutFile "target\release\RIGStats_${VERSION}_x64-setup.exe"
InstallDir "$PROGRAMFILES64\RIGStats"
InstallDirRegKey HKLM "Software\RIGStats" "InstallDir"

; ── Over-the-shoulder UAC handling (administrator broker model) ────────────────
; The installer starts as a *normal user* process and elevates an inner instance
; only for the machine-wide work (driver, service, Program Files, HKLM). All
; per-user actions and the final app launch are delegated back to the original
; unelevated user via the UAC plugin's UAC_AsUser_ExecShell. This guarantees
; RIGStats is installed/launched for the user actually sitting at the machine,
; even when a *different* administrator account approves the UAC prompt (e.g. a
; parent elevating for a child's standard account). See build/nsis-plugins/.
RequestExecutionLevel user

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

; Vendored UAC plugin (zlib license) — pinned under build/nsis-plugins.
; Paths are relative to the repo root (see !cd .. above).
!addplugindir "build\nsis-plugins\x86-unicode"
!include "build\nsis-plugins\UAC.nsh"

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

; Launch RIGStats as the original (unelevated) user via the UAC plugin so the
; app runs with normal privileges and all per-user state (settings, tray, the
; in-app autostart toggle's HKCU key) is created for the user at the machine —
; not the administrator who approved the UAC prompt.
Function LaunchRIGStats
  !insertmacro UAC_AsUser_ExecShell "open" "$INSTDIR\rigstats.exe" "" "$INSTDIR" ""
FunctionEnd

; ── Shared elevation macro (installer + uninstaller) ───────────────────────────
; Elevates an inner instance and routes per-user actions through the outer
; (unelevated) process. After elevation succeeds, switches shell-var context to
; "all users" so shortcuts land in the All-Users Start Menu / Public Desktop and
; are visible to every account (including the standard user who launched setup).
!macro RIGStatsInit thing
  uac_tryagain:
  !insertmacro UAC_RunElevated
  ${Switch} $0
  ${Case} 0
    ${IfThen} $1 = 1 ${|} Quit ${|}        ; outer process; inner finished — done.
    ${IfThen} $3 <> 0 ${|} ${Break} ${|}   ; we are admin — proceed with the work.
    ${If} $1 = 3                           ; RunAs ok, but the supplied user is not admin.
      MessageBox mb_YesNo|mb_IconExclamation|mb_TopMost|mb_SetForeground \
        "RIGStats ${thing} requires administrator privileges. Try again with an administrator account?" \
        /SD IDNO IDYES uac_tryagain IDNO 0
    ${EndIf}
    ; fall through to the abort message below.
  ${Case} 1223
    MessageBox mb_IconStop|mb_TopMost|mb_SetForeground \
      "RIGStats ${thing} requires administrator privileges. Aborting." /SD IDOK
    Quit
  ${Case} 1062
    MessageBox mb_IconStop|mb_TopMost|mb_SetForeground \
      "Logon service not running. Aborting." /SD IDOK
    Quit
  ${Default}
    MessageBox mb_IconStop|mb_TopMost|mb_SetForeground \
      "Unable to elevate, error $0." /SD IDOK
    Quit
  ${EndSwitch}
  SetShellVarContext all
!macroend

; ── Pre-install ────────────────────────────────────────────────────────────────
Function .onInit
  ; Detect /autoupdate flag: show progress window but skip wizard pages.
  ; (The UAC plugin forwards the original command line to the elevated instance,
  ; so both the outer and inner process observe the flag.)
  ${GetOptions} $CMDLINE "/autoupdate" $R0
  ${IfNot} ${Errors}
    StrCpy $AutoUpdate 1
    SetAutoClose true
  ${EndIf}

  ; Elevate. The outer (user) process returns here only to Quit once the inner
  ; (admin) process has finished, so everything below runs elevated.
  !insertmacro RIGStatsInit "installation"

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
  File "sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\rigstats-sensor.exe"
  ; Native libs that may be emitted alongside the single-file exe depending on
  ; the .NET runtime pack. They are Unix P/Invoke helpers never loaded on
  ; Windows, so /nonfatal keeps the build working whether or not they appear.
  File /nonfatal "sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\MonoPosixHelper.dll"
  File /nonfatal "sensor-sidecar\bin\Release\net10.0-windows\win-x64\publish\libMonoPosixHelper.dll"
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
    "DisplayIcon"      "$INSTDIR\rigstats.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "Publisher"        "codeby.se"
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "NoModify"         1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats" \
    "NoRepair"         1

  ; ── Shortcuts (All-Users, via SetShellVarContext all set in .onInit) ──────
  ; Remove stale per-user shortcuts left by pre-UAC installers, which created
  ; them in the elevating administrator's profile instead of a shared location.
  ; Safe: only RIGStats-named items under each user's per-user Start Menu/Desktop.
  nsExec::ExecToLog 'cmd /C for /D %D in ("%SystemDrive%\Users\*") do (del /F /Q "%D\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\RIGStats\*.lnk" >NUL 2>&1 & rmdir "%D\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\RIGStats" >NUL 2>&1 & del /F /Q "%D\Desktop\RIGStats.lnk" >NUL 2>&1)'

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
  FileWrite $9 "service_create_exit=$4$\r$\n"
  FileWrite $9 "service_start_exit=$6$\r$\n"
  FileClose $9

  ; ── Auto-launch with update notification (in-app /autoupdate installs only) ──
  ; Manual installs use the finish-page "Launch RIGStats" checkbox instead.
  ; Launched as the original unelevated user so settings/tray bind to that user
  ; (UAC_AsUser_ExecShell preserves the --just-updated argument, unlike a plain
  ; explorer.exe relaunch).
  ${If} $AutoUpdate == 1
    !insertmacro UAC_AsUser_ExecShell "open" "$INSTDIR\rigstats.exe" "--just-updated=${VERSION}" "$INSTDIR" ""
  ${EndIf}
SectionEnd

; ── Optional: desktop shortcut ────────────────────────────────────────────────
Section /o "Desktop Shortcut" SecDesktop
  CreateShortcut "$DESKTOP\RIGStats.lnk" "$INSTDIR\rigstats.exe"
SectionEnd

; ── Uninstaller ────────────────────────────────────────────────────────────────
Function un.onInit
  ; Elevate the uninstaller (needs admin for sc delete + driver removal) and
  ; switch to All-Users shell-var context so the shared shortcuts are removed.
  !insertmacro RIGStatsInit "uninstallation"
FunctionEnd

Section "Uninstall"
  nsExec::ExecToLog 'cmd /C sc stop rigstats-sensor >NUL 2>&1'
  Sleep 2000
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'

  ; Kill the main app if running.
  nsExec::ExecToLog 'cmd /C taskkill /F /IM rigstats.exe >NUL 2>&1'
  Sleep 1000

  Delete "$INSTDIR\rigstats.exe"
  Delete "$INSTDIR\rigstats-sensor.exe"
  Delete "$INSTDIR\MonoPosixHelper.dll"
  Delete "$INSTDIR\libMonoPosixHelper.dll"
  Delete "$INSTDIR\CHANGELOG.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir /r "$INSTDIR\pawnio"
  RMDir "$INSTDIR"

  ; All-Users shortcuts (un.onInit set SetShellVarContext all).
  Delete "$DESKTOP\RIGStats.lnk"
  Delete "$SMPROGRAMS\RIGStats\RIGStats.lnk"
  Delete "$SMPROGRAMS\RIGStats\Uninstall RIGStats.lnk"
  RMDir  "$SMPROGRAMS\RIGStats"

  ; Remove any stale per-user shortcuts from pre-UAC installers.
  nsExec::ExecToLog 'cmd /C for /D %D in ("%SystemDrive%\Users\*") do (del /F /Q "%D\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\RIGStats\*.lnk" >NUL 2>&1 & rmdir "%D\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\RIGStats" >NUL 2>&1 & del /F /Q "%D\Desktop\RIGStats.lnk" >NUL 2>&1)'

  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\RIGStats"
  DeleteRegKey HKLM "Software\RIGStats"

  ; Remove old LHM tasks if still present.
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
SectionEnd
