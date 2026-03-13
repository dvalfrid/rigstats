!include "FileFunc.nsh"

!macro customInstall
  ; Prefer an existing LibreHardwareMonitor installation if present.
  ; Fallback to bundled LHM inside RigDashboard resources.
  StrCpy $0 "$INSTDIR\\resources\\lhm\\LibreHardwareMonitor.exe"

  IfFileExists "$PROGRAMFILES64\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$PROGRAMFILES64\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  IfFileExists "$PROGRAMFILES\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$PROGRAMFILES\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  IfFileExists "$LOCALAPPDATA\\Programs\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$LOCALAPPDATA\\Programs\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  ; Apply default config to the selected LHM installation (existing or bundled).
  ; This enables the web server on port 8085 without manual setup.
  IfFileExists "$INSTDIR\\resources\\lhm\\defaults\\LibreHardwareMonitor.config" 0 +9
  ${GetParent} "$0" $1
  IfFileExists "$1\\LibreHardwareMonitor.config" 0 +3
  Delete "$1\\LibreHardwareMonitor.config.backup"
  Rename "$1\\LibreHardwareMonitor.config" "$1\\LibreHardwareMonitor.config.backup"
  CopyFiles /SILENT "$INSTDIR\\resources\\lhm\\defaults\\LibreHardwareMonitor.config" "$1\\LibreHardwareMonitor.config"

  ; Create or update scheduled task for LibreHardwareMonitor at user logon.
  ; /IT keeps it interactive in user session, /RL HIGHEST grants elevated access.
  nsExec::ExecToLog 'schtasks /Create /TN "RigStats\\LibreHardwareMonitor" /TR "$0" /SC ONLOGON /RL HIGHEST /F /IT'

  ; Start the task once directly after install so sensors are available immediately.
  nsExec::ExecToLog 'schtasks /Run /TN "RigStats\\LibreHardwareMonitor"'
!macroend

!macro customUnInstall
  ; Remove scheduled task created during installation.
  nsExec::ExecToLog 'schtasks /Delete /TN "RigStats\\LibreHardwareMonitor" /F'
!macroend
