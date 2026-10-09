// PreToolUse (Bash/PowerShell): while tools/dev-sidecar.ps1 runs, the debug
// sidecar exe in sensor-sidecar/bin/Debug is locked, so building the sidecar
// fails with a confusing file-in-use error. Block those commands with a
// clear message instead. Signal: a rigstats-sensor process is running while
// the rigstats-sensor service is not (the script stops the service first).
const { execFileSync } = require('child_process');

let input = '';
process.stdin.on('data', (d) => (input += d));
process.stdin.on('end', () => {
  let command = '';
  try {
    command = JSON.parse(input).tool_input?.command ?? '';
  } catch {
    process.exit(0);
  }
  // Only Debug builds write to the locked bin/Debug; `-c Release` and
  // `cargo xtask verify/build` (Release) don't, so they run meanwhile.
  const debugBuild = /dotnet\s+(build|test|run)[^\n]*sensor-sidecar/.test(command) &&
    !/(-c|--configuration)\s+Release/i.test(command);
  if (!debugBuild) process.exit(0);

  let devSidecar = false;
  try {
    const out = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      "$s = Get-Service rigstats-sensor -ErrorAction SilentlyContinue; " +
      "$p = Get-Process rigstats-sensor -ErrorAction SilentlyContinue; " +
      "if ($p -and (-not $s -or $s.Status -ne 'Running')) { 'dev' }"], { encoding: 'utf8', timeout: 10000 });
    devSidecar = out.trim() === 'dev';
  } catch {
    process.exit(0); // can't tell — don't block.
  }
  if (devSidecar) {
    process.stderr.write('The dev sidecar (tools/dev-sidecar.ps1) is running and locks the sidecar build output. ' +
      'Ask the owner to stop it with Ctrl+C in its elevated window, then run the build again.\n');
    process.exit(2);
  }
  process.exit(0);
});
