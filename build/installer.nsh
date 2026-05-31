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
  ; Open install log in ProgramData — neither $PROGRAMDATA nor $COMMONAPPDATA are
  ; valid NSIS built-in variables; use ReadEnvStr to read the Windows env var instead.
  ReadEnvStr $R2 "PROGRAMDATA"
  CreateDirectory "$R2\se.codeby.rigstats"
  FileOpen $9 "$R2\se.codeby.rigstats\rigstats-install.log" w
  FileWrite $9 "[RIGStats post-install]$\r$\n"
  FileWrite $9 "version=${VERSION}$\r$\n"
  FileWrite $9 "instdir=$INSTDIR$\r$\n"
  nsExec::ExecToStack 'cmd /C echo %DATE% %TIME%'
  Pop $R3
  Pop $R4
  FileWrite $9 "timestamp=$R4$\r$\n"

  ; Remove old LHM scheduled tasks from pre-sidecar versions (< 1.20).
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RIGStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "RigStats\LibreHardwareMonitor" /F >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C schtasks /Delete /TN "LibreHardwareMonitor" /F >NUL 2>&1'
  FileWrite $9 "old_lhm_tasks_removed=1$\r$\n"

  ; Remove the bundled LHM directory left over from pre-sidecar versions (< 1.20).
  ; The process was already killed in PREINSTALL so the files are no longer locked.
  RMDir /r "$INSTDIR\lhm"
  FileWrite $9 "old_lhm_dir_removed=1$\r$\n"

  ; Verify pawnio.inf exists before attempting installation.
  IfFileExists "$INSTDIR\pawnio\pawnio.inf" pawnio_inf_ok pawnio_inf_missing
  pawnio_inf_ok:
    FileWrite $9 "pawnio_inf_exists=1$\r$\n"
    Goto pawnio_install
  pawnio_inf_missing:
    FileWrite $9 "pawnio_inf_exists=0$\r$\n"
  pawnio_install:

  ; Install PawnIO kernel driver (used by LibreHardwareMonitorLib for sensor access).
  ; pnputil stages the signed driver into the Windows Driver Store and registers the
  ; service. Safe to run on reinstall — pnputil silently skips already-staged packages.
  nsExec::ExecToStack '"$WINDIR\Sysnative\pnputil.exe" /add-driver "$INSTDIR\pawnio\pawnio.inf" /install'
  Pop $R0
  Pop $R1
  DetailPrint "PawnIO install: exit $R0"
  FileWrite $9 "pawnio_install_exit=$R0$\r$\n"
  FileWrite $9 "pawnio_install_output=$R1$\r$\n"

  ; Exit 0 = newly added, 259 = already present — both are success.
  ; Anything else is a real failure; log enum-drivers for diagnostics.
  IntCmp $R0 0 pawnio_done pawnio_nonzero pawnio_nonzero
  pawnio_nonzero:
    IntCmp $R0 259 pawnio_done pawnio_check_existing pawnio_check_existing
  pawnio_check_existing:
    nsExec::ExecToStack '"$WINDIR\Sysnative\pnputil.exe" /enum-drivers'
    Pop $R3
    Pop $R4
    FileWrite $9 "pawnio_already_staged=$R4$\r$\n"
  pawnio_done:

  ; Remove the existing service before re-registering (handles update scenario).
  ; The service was already stopped in PREINSTALL; this delete just removes the
  ; SCM entry so we can re-create it with the fresh binary path.
  nsExec::ExecToLog 'cmd /C sc delete rigstats-sensor >NUL 2>&1'
  Sleep 1000

  ; Register rigstats-sensor.exe as a Windows Service running as LocalSystem.
  ; LocalSystem has the privileges needed to load the PawnIO kernel driver.
  ; start= auto means the SCM starts it automatically at boot.
  nsExec::ExecToStack 'cmd /C sc create rigstats-sensor binPath= "$INSTDIR\rigstats-sensor.exe" start= auto obj= LocalSystem displayname= "RIGStats Sensor"'
  Pop $4
  Pop $5
  DetailPrint "Service create: exit $4"
  FileWrite $9 "service_create_exit=$4$\r$\n"
  FileWrite $9 "service_create_output=$5$\r$\n"

  nsExec::ExecToLog 'cmd /C sc description rigstats-sensor "Reads hardware sensors for the RIGStats dashboard." >NUL 2>&1'
  nsExec::ExecToLog 'cmd /C sc failure rigstats-sensor reset= 60 actions= restart/5000/restart/10000/restart/30000 >NUL 2>&1'

  ; Start the service now so sensor data is available without a reboot.
  nsExec::ExecToStack 'cmd /C sc start rigstats-sensor >NUL 2>&1'
  Pop $6
  Pop $7
  DetailPrint "Service start: exit $6"
  FileWrite $9 "service_start_exit=$6$\r$\n"

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
