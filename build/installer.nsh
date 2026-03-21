!include "FileFunc.nsh"

!macro NSIS_HOOK_POSTINSTALL
  ; Open install log in the app data directory so it can be included in diagnostics.
  CreateDirectory "$APPDATA\se.codeby.rigstats"
  FileOpen $9 "$APPDATA\se.codeby.rigstats\rigstats-install.log" w
  FileWrite $9 "[RIGStats post-install]$\r$\n"

  ; Prefer an existing LibreHardwareMonitor installation if present.
  ; Fallback to bundled LHM inside RigStats installation directory.
  StrCpy $0 "$INSTDIR\\lhm\\LibreHardwareMonitor.exe"

  IfFileExists "$PROGRAMFILES64\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$PROGRAMFILES64\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  IfFileExists "$PROGRAMFILES\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$PROGRAMFILES\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  IfFileExists "$LOCALAPPDATA\\Programs\\LibreHardwareMonitor\\LibreHardwareMonitor.exe" 0 +2
  StrCpy $0 "$LOCALAPPDATA\\Programs\\LibreHardwareMonitor\\LibreHardwareMonitor.exe"

  FileWrite $9 "lhm_exe=$0$\r$\n"

  ; Apply default config to the selected LHM installation (existing or bundled).
  ; This enables the web server on port 8085 without manual setup.
  IfFileExists "$INSTDIR\\lhm\\defaults\\LibreHardwareMonitor.config" 0 no_bundled_config
  ${GetParent} "$0" $1
  IfFileExists "$1\\LibreHardwareMonitor.config" 0 +3
  Delete "$1\\LibreHardwareMonitor.config.backup"
  Rename "$1\\LibreHardwareMonitor.config" "$1\\LibreHardwareMonitor.config.backup"
  nsExec::ExecToStack 'cmd /C copy /Y "$INSTDIR\lhm\defaults\LibreHardwareMonitor.config" "$1\LibreHardwareMonitor.config"'
  Pop $4
  Pop $5
  DetailPrint "LHM config copy: exit $4"
  FileWrite $9 "config_copy_exit=$4$\r$\n"
  no_bundled_config:

  ; Create or update scheduled task for LibreHardwareMonitor at any user logon.
  ; Uses PowerShell Register-ScheduledTask without -UserId so the trigger fires for
  ; ALL users (not just the admin who ran the installer). HighestAvailable = admin token
  ; for admin users, standard token for standard users.
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RIGStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'

  FileOpen $3 "$TEMP\create-lhm-task.ps1" w
  FileWrite $3 "$$a = New-ScheduledTaskAction -Execute $\"$0$\"$\r$\n"
  FileWrite $3 "$$t = New-ScheduledTaskTrigger -AtLogOn$\r$\n"
  FileWrite $3 "$$s = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew -ExecutionTimeLimit ([TimeSpan]::Zero)$\r$\n"
  FileWrite $3 "$$p = New-ScheduledTaskPrincipal -GroupId 'S-1-5-32-545' -RunLevel Highest$\r$\n"
  FileWrite $3 "Register-ScheduledTask -TaskName 'LibreHardwareMonitor' -Action $$a -Trigger $$t -Settings $$s -Principal $$p -Force$\r$\n"
  FileClose $3
  nsExec::ExecToStack 'powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$TEMP\create-lhm-task.ps1"'
  Pop $4
  Pop $5
  DetailPrint "LHM task register: exit $4"
  FileWrite $9 "lhm_task_register_exit=$4$\r$\n"
  Delete "$TEMP\create-lhm-task.ps1"

  ; Run LHM directly in the installer's admin context so PawnIO (kernel driver) is
  ; installed on first use. The user will see a PawnIO prompt and should click Yes.
  ; Using Exec (non-blocking) so the installer can finish while LHM initialises.
  DetailPrint "Starting LibreHardwareMonitor — click Yes if asked about PawnIO installation."
  FileWrite $9 "lhm_started=1$\r$\n"
  Exec "$\"$0$\""

  FileClose $9
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Remove scheduled task created during installation.
  nsExec::ExecToLog 'schtasks /Delete /TN "RigStats\\LibreHardwareMonitor" /F'
  nsExec::ExecToLog 'schtasks /Delete /TN "LibreHardwareMonitor" /F'
!macroend
