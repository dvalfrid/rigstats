!macro NSIS_HOOK_PREINSTALL
  ; Stop the sidecar service before files are extracted.
  ; During an update the running service holds rigstats-sensor.exe open — without
  ; this the installer fails with "Error opening file for writing".
  ; All commands redirect to NUL so they fail silently on a fresh install.
  nsExec::ExecToLog 'cmd /C sc stop rigstats-sensor >NUL 2>&1'
  Sleep 3000
  ; Also stop old LHM processes when upgrading from a pre-sidecar version (< 1.20).
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "RIGStats\LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "RigStats\LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /End /TN "LibreHardwareMonitor" >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C taskkill /F /IM LibreHardwareMonitor.exe >NUL 2>&1'
  Sleep 1000
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Open install log in ProgramData — the installer runs elevated (perMachine) so
  ; $APPDATA resolves to the system account profile, not the user's. $PROGRAMDATA
  ; is machine-wide and consistent regardless of which account runs the installer.
  CreateDirectory "$PROGRAMDATA\se.codeby.rigstats"
  FileOpen $9 "$PROGRAMDATA\se.codeby.rigstats\rigstats-install.log" w
  FileWrite $9 "[RIGStats post-install]$\r$\n"

  ; Remove old LHM scheduled tasks from pre-sidecar versions (< 1.20).
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RIGStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
  FileWrite $9 "old_lhm_tasks_removed=1$\r$\n"

  ; Remove the existing service before re-registering (handles update scenario).
  ; The service was already stopped in PREINSTALL; this delete just removes the
  ; SCM entry so we can re-create it with the fresh binary path.
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'
  Sleep 1000

  ; Register rigstats-sensor.exe as a Windows Service running as LocalSystem.
  ; LocalSystem has the privileges needed to load the WinRing0 kernel driver.
  ; start= auto means the SCM starts it automatically at boot.
  nsExec::ExecToStack 'cmd /C sc create rigstats-sensor binPath= "$INSTDIR\rigstats-sensor.exe" start= auto obj= LocalSystem displayname= "RIGStats Sensor"'
  Pop $4
  Pop $5
  DetailPrint "Service create: exit $4"
  FileWrite $9 "service_create_exit=$4$\r$\n"

  nsExec::ExecToLog 'cmd /C sc description rigstats-sensor "Reads hardware sensors for the RIGStats dashboard." >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C sc failure rigstats-sensor reset= 60 actions= restart/5000/restart/10000/restart/30000 >NUL 2>&1'

  ; Start the service now so sensor data is available without a reboot.
  nsExec::ExecToStack 'cmd /C sc start rigstats-sensor >NUL 2>&1'
  Pop $6
  Pop $7
  DetailPrint "Service start: exit $6"
  FileWrite $9 "service_start_exit=$6$\r$\n"

  ; Add a Windows Defender exclusion for the sidecar so WinRing0 (the kernel
  ; driver used by LibreHardwareMonitorLib for Super I/O sensor access) does
  ; not trigger a SmartScreen or real-time protection alert on first launch.
  nsExec::ExecToLog 'cmd /C powershell -NonInteractive -Command "Add-MpPreference -ExclusionProcess ''rigstats-sensor.exe'' -ErrorAction SilentlyContinue" >NUL 2>&1'
  FileWrite $9 "defender_exclusion=added$\r$\n"

  FileClose $9
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Stop and remove the sensor service.
  nsExec::ExecToLog 'cmd /C sc stop rigstats-sensor >NUL 2>&1'
  Sleep 2000
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'
  ; Remove old LHM tasks if still present from a partial upgrade.
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
!macroend
