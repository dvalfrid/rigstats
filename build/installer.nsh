!include "FileFunc.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; Prefer an existing LibreHardwareMonitor installation if present.
  ; Fallback to bundled LHM inside RigStats installation directory.
  StrCpy $0 "$INSTDIR\\lhm\\LibreHardwareMonitor.exe"

  IfFileExists "$PROGRAMFILES64\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$PROGRAMFILES64\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  IfFileExists "$PROGRAMFILES\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$PROGRAMFILES\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  IfFileExists "$LOCALAPPDATA\\Programs\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$LOCALAPPDATA\\Programs\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  ; Apply default config to the selected LHM installation (existing or bundled).
  ; This enables the web server on port 8085 without manual setup.
  IfFileExists "$INSTDIR\\lhm\\defaults\\LibreHardwareMonitor.config" 0 +9
  ${GetParent} "$0" $1
  IfFileExists "$1\\LibreHardwareMonitor.config" 0 +3
  Delete "$1\\LibreHardwareMonitor.config.backup"
  Rename "$1\\LibreHardwareMonitor.config" "$1\\LibreHardwareMonitor.config.backup"
  nsExec::ExecToLog 'cmd /C copy /Y "$INSTDIR\lhm\defaults\LibreHardwareMonitor.config" "$1\LibreHardwareMonitor.config"'

  ; Create or update scheduled task for LibreHardwareMonitor at user logon.
  ; /IT keeps it interactive in user session, /RL HIGHEST grants elevated access.
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'schtasks /Create /TN "LibreHardwareMonitor" /TR "\"$0\"" /SC ONLOGON /RL HIGHEST /F /IT'
  Pop $2

  ; Some environments deny HIGHEST at install time. Fallback to LIMITED so task still exists.
  StrCmp $2 "0" +2 0
  nsExec::ExecToLog 'schtasks /Create /TN "LibreHardwareMonitor" /TR "\"$0\"" /SC ONLOGON /RL LIMITED /F /IT'

  ; Start the task once directly after install so sensors are available immediately.
  nsExec::ExecToLog 'schtasks /Run /TN "LibreHardwareMonitor"'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove scheduled task created during installation.
  nsExec::ExecToLog 'schtasks /Delete /TN "RigStats\\LibreHardwareMonitor" /F'
  nsExec::ExecToLog 'schtasks /Delete /TN "LibreHardwareMonitor" /F'
!macroend
