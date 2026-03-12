const { app, BrowserWindow, screen, ipcMain, powerSaveBlocker } = require('electron');
const os = require('os');
const path = require('path');
const si = require('systeminformation');

let mainWindow;
let powerBlockerId = null;

// ── Find the target display ─────────────────────────────────────
function findDashboardDisplay() {
  const displays = screen.getAllDisplays();
  console.log('Available displays:');
  displays.forEach((d, i) => {
    console.log(`  [${i}] ${d.size.width}x${d.size.height} @ (${d.bounds.x},${d.bounds.y})`);
  });
  const target = displays.find(d => d.size.width === 450 && d.size.height === 1920);
  if (target) { console.log('✓ Found 450x1920 display'); return target; }
  if (displays.length > 1) { console.log('↩ Using secondary display as fallback'); return displays[1]; }
  return null;
}

function createWindow() {
  const targetDisplay = findDashboardDisplay();
  const iconPath = path.join(__dirname, 'assets', 'icon.ico');
  const windowConfig = {
    width: 450, height: 1920,
    frame: false, resizable: false,
    alwaysOnTop: false,
    icon: iconPath,
    backgroundColor: '#06070a',
    webPreferences: { nodeIntegration: true, contextIsolation: false }
  };
  if (targetDisplay) {
    windowConfig.x = targetDisplay.bounds.x;
    windowConfig.y = targetDisplay.bounds.y;
  }
  mainWindow = new BrowserWindow(windowConfig);
  mainWindow.loadFile(path.join(__dirname, 'src', 'index.html'));
  if (targetDisplay) {
    mainWindow.setBounds({
      x: targetDisplay.bounds.x, y: targetDisplay.bounds.y,
      width: targetDisplay.size.width, height: targetDisplay.size.height
    });
  }
}

app.whenReady().then(() => {
  if (process.platform === 'win32') {
    app.setAppUserModelId('com.rigdashboard.app');
  }
  createWindow();
  powerBlockerId = powerSaveBlocker.start('prevent-display-sleep');
  console.log('✓ Display sleep mode blocked');
});
app.on('window-all-closed', () => {
  if (powerBlockerId !== null) {
    powerSaveBlocker.stop(powerBlockerId);
    console.log('✓ Display sleep mode block stopped');
  }
  if (process.platform !== 'darwin') app.quit();
});

// ── Fetch data from LibreHardwareMonitor web API ─────────────────
// LibreHardwareMonitor runs a local server at http://localhost:8085
async function fetchLHM() {
  try {
    const response = await fetch('http://localhost:8085/data.json', {
      signal: AbortSignal.timeout(800)
    });
    const json = await response.json();
    return parseLHM(json);
  } catch(e) {
    return null; // LHM is not running
  }
}

// Recursively walk LHM's tree structure
// parent = nearest parent label
function flattenLHM(node, results = [], parent = '') {
  const myText = node.Text || node.text || '';
  const myVal  = node.Value || node.value || '';
  if (myVal !== '' && myVal !== 'Value') {
    results.push({ text: myText, value: myVal, parent });
  }
  const children = node.Children || node.children || [];
  children.forEach(c => flattenLHM(c, results, myText || parent));
  return results;
}

let debugDumped = false;

// Parse LHM values: "56,0 °C" -> 56.0, "8,0 %" -> 8.0, "3540,0 MB" -> 3540.0
function parseVal(str) {
  if (!str || str.trim() === '') return null;
  const clean = str.replace(',', '.').replace(/[^\d.-]/g, '');
  const v = parseFloat(clean);
  return isNaN(v) ? null : v;
}

// Find sensor under a specific parent node, matched by text
function findUnder(nodes, parentName, textMatch) {
  const match = nodes.find(n =>
    n.parent.toLowerCase().includes(parentName.toLowerCase()) &&
    n.text.toLowerCase().includes(textMatch.toLowerCase())
  );
  return match ? parseVal(match.value) : null;
}

// Find Nth match (for sensors with duplicate names, e.g. "GPU Core")
function findNth(nodes, parentName, textMatch, nth = 0) {
  const matches = nodes.filter(n =>
    n.parent.toLowerCase().includes(parentName.toLowerCase()) &&
    n.text.toLowerCase().includes(textMatch.toLowerCase())
  );
  return matches[nth] ? parseVal(matches[nth].value) : null;
}

function parseLHM(data) {
  const nodes = flattenLHM(data);

  // ── Find RX 9070 XT sensors ─────────────────────────────────────
  // LHM has two GPU devices (iGPU + dGPU) with identical parent names.
  // We identify RX 9070 XT by "GPU Memory Total" ~= 16304 MB.
  // dGPU sensors are in a contiguous block in nodes[].
  // Strategy: find dGPU "GPU Memory Total" index and scan around it.

  const vramTotalIdx = nodes.findIndex(n =>
    n.text === 'GPU Memory Total' && parseVal(n.value) > 10000
  );

  // Build a window of nodes around dGPU memory node (+/- 30 nodes)
  const gpuBlock = vramTotalIdx >= 0
    ? nodes.slice(Math.max(0, vramTotalIdx - 25), vramTotalIdx + 5)
    : [];

  // Helper: read value in gpuBlock by parent + text
  function gpuFind(parentMatch, textMatch) {
    const n = gpuBlock.find(n =>
      n.parent === parentMatch && n.text === textMatch
    );
    return n ? parseVal(n.value) : null;
  }

  // Read GPU sensors from the block
  const gpuTemp     = gpuFind('Temperatures', 'GPU Core');
  const gpuHotspot  = gpuFind('Temperatures', 'GPU Hot Spot');
  const gpuMemTemp  = gpuFind('Temperatures', 'GPU Memory');
  const gpuLoad     = gpuFind('Load', 'GPU Core');
  const gpuPower    = gpuFind('Powers', 'GPU Package');
  const gpuFreq     = gpuFind('Clocks', 'GPU Core');
  const gpuFan      = gpuFind('Fans', 'GPU Fan');
  const vramUsed    = gpuFind('Data', 'GPU Memory Used');
  const vramTotal   = gpuFind('Data', 'GPU Memory Total');

  // ── CPU ─────────────────────────────────────────────────────────
  const cpuTempNode  = nodes.find(n => n.text === 'Core (Tctl/Tdie)');
  const cpuPowerNode = nodes.find(n => n.parent === 'Powers' && n.text === 'Package');
  const cpuTemp  = cpuTempNode  ? parseVal(cpuTempNode.value)  : null;
  const cpuPower = cpuPowerNode ? parseVal(cpuPowerNode.value) : null;

  // ── DISK — read from Throughput nodes ───────────────────────────
  // First Throughput Read Rate = NVMe 1 (C:), second = NVMe 2 (D:)
  const readNodes  = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Read Rate');
  const writeNodes = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Write Rate');
  // Convert "54,9 MB/s" or "614,2 KB/s" to MB/s
  function toMBs(str) {
    if (!str) return 0;
    const v = parseVal(str);
    if (str.includes('KB')) return v / 1024;
    if (str.includes('GB')) return v * 1024;
    return v; // MB/s
  }
  const disk1Read  = readNodes[0]  ? toMBs(readNodes[0].value)  : 0;
  const disk1Write = writeNodes[0] ? toMBs(writeNodes[0].value) : 0;
  const disk2Read  = readNodes[1]  ? toMBs(readNodes[1].value)  : 0;
  const disk2Write = writeNodes[1] ? toMBs(writeNodes[1].value) : 0;
  const totalRead  = disk1Read + disk2Read;
  const totalWrite = disk1Write + disk2Write;

  // ── NETWORK — read from Throughput Upload/Download ─────────────
  // Pick the interface with the highest traffic
  const uploadNodes   = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Upload Speed');
  const downloadNodes = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Download Speed');
  let bestUp = 0, bestDown = 0;
  uploadNodes.forEach((n, i) => {
    const up   = toMBs(n.value) * 8; // MB/s → Mbps
    const down = downloadNodes[i] ? toMBs(downloadNodes[i].value) * 8 : 0;
    if (up + down > bestUp + bestDown) { bestUp = up; bestDown = down; }
  });

  // ── NVMe temp ────────────────────────────────────────────────────
  const nvmeTempNodes = nodes.filter(n => n.text === 'Composite Temperature');
  const nvme1Temp = nvmeTempNodes[0] ? parseVal(nvmeTempNodes[0].value) : null;
  const nvme2Temp = nvmeTempNodes[1] ? parseVal(nvmeTempNodes[1].value) : null;

  return {
    gpuLoad, gpuTemp, gpuHotspot, gpuMemTemp, gpuFreq, gpuPower, gpuFan,
    vramUsed, vramTotal,
    cpuTemp, cpuPower,
    diskRead: totalRead, diskWrite: totalWrite,
    disk1Read, disk1Write, disk2Read, disk2Write,
    netUp: bestUp, netDown: bestDown,
    nvme1Temp, nvme2Temp
  };
}

// ── IPC: fetch all stats ──────────────────────────────────────────
ipcMain.handle('get-stats', async () => {
  try {
    const [cpuLoad, cpuCurrentSpeed, mem, networkStats, fsSize, lhm, memLayout] = await Promise.all([
      si.currentLoad(),
      si.cpuCurrentSpeed(),
      si.mem(),
      si.networkStats(),
      si.fsSize(),
      fetchLHM(),
      si.memLayout()
    ]);

    // RAM spec (type + speed + module count), e.g. "DDR5 6000 MT/s (2 DIMMs)"
    const dimms = (memLayout || []).filter(m => (m.size || 0) > 0);
    const ramTypes = [...new Set(dimms.map(m => m.type).filter(Boolean))];
    const speedCandidates = dimms
      .map(m => Number(m.clockSpeed || m.frequency || m.configuredClockSpeed || 0))
      .filter(v => Number.isFinite(v) && v > 0);
    const maxSpeed = speedCandidates.length ? Math.max(...speedCandidates) : null;

    let ramSpec = ramTypes.length ? ramTypes.join('/') : 'RAM';
    if (maxSpeed) ramSpec += ` ${Math.round(maxSpeed)} MT/s`;
    if (dimms.length) ramSpec += ` (${dimms.length} DIMMs)`;

    // Disk — use LHM Throughput data if available, otherwise fsStats
    let diskRead = lhm?.diskRead ?? 0;
    let diskWrite = lhm?.diskWrite ?? 0;
    if (!lhm) {
      try {
        const fs = await si.fsStats();
        if (fs) { diskRead = (fs.rx_sec || 0) / 1e6; diskWrite = (fs.wx_sec || 0) / 1e6; }
      } catch(e) {}
    }

    // Network — use LHM if available, otherwise systeminformation
    let netUp   = lhm?.netUp   ?? 0;  // Mbps
    let netDown = lhm?.netDown ?? 0;
    let netIface = '—';
    if (!lhm) {
      const activeNet = (networkStats || []).sort((a, b) =>
        (b.rx_sec + b.tx_sec) - (a.rx_sec + a.tx_sec)
      )[0] || {};
      netUp    = (activeNet.tx_sec || 0) * 8 / 1e6;
      netDown  = (activeNet.rx_sec || 0) * 8 / 1e6;
      netIface = activeNet.iface || '—';
    }

    const drives = (fsSize || []).filter(f => f.size > 1e9);
    const vramUsedMB  = lhm?.vramUsed  ?? null;
    const vramTotalMB = lhm?.vramTotal ?? 16384;

    return {
      cpu: {
        load:  Math.round(cpuLoad.currentLoad),
        cores: cpuLoad.cpus.map(c => Math.round(c.load)),
        temp:  lhm?.cpuTemp  ?? null,
        freq:  cpuCurrentSpeed.avg || 0,
        power: lhm?.cpuPower ?? null
      },
      gpu: {
        load:      lhm?.gpuLoad     ?? null,
        temp:      lhm?.gpuTemp     ?? null,
        hotspot:   lhm?.gpuHotspot  ?? null,
        freq:      lhm?.gpuFreq     ?? null,
        vramUsed:  vramUsedMB,
        vramTotal: vramTotalMB,
        fanSpeed:  lhm?.gpuFan      ?? null,
        power:     lhm?.gpuPower    ?? null
      },
      ram: {
        total: mem.total,
        used:  mem.used,
        free:  mem.free,
        spec:  ramSpec
      },
      net: {
        up:    netUp,
        down:  netDown,
        iface: netIface
      },
      disk: {
        read:   diskRead,
        write:  diskWrite,
        drives: drives.map(d => ({
          fs:   d.fs,
          size: d.size,
          used: d.used,
          pct:  Math.round(d.use || 0)
        }))
      },
      lhmConnected: lhm !== null
    };
  } catch (err) {
    console.error('Stats error:', err);
    return null;
  }
});

ipcMain.handle('set-autostart', (_, enable) => {
  app.setLoginItemSettings({ openAtLogin: enable, openAsHidden: false, name: 'RigDashboard' });
});
ipcMain.handle('get-autostart', () => app.getLoginItemSettings().openAtLogin);
ipcMain.handle('get-system-name', () => os.hostname());
ipcMain.handle('get-cpu-info', async () => {
  const cpu = await si.cpu();
  return `${cpu.manufacturer} ${cpu.brand}`.trim();
});
ipcMain.handle('get-gpu-info', async () => {
  const gpus = await si.graphics();
  const gpu = (gpus.controllers || []).find(g => g.vram > 8000) || gpus.controllers?.[0];
  return gpu ? gpu.model : null;
});