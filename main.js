const { app, BrowserWindow, screen, ipcMain, powerSaveBlocker } = require('electron');
const os = require('os');
const path = require('path');
const si = require('systeminformation');

let mainWindow;
let powerBlockerId = null;

// ── Hitta rätt skärm ─────────────────────────────────────────────
function findDashboardDisplay() {
  const displays = screen.getAllDisplays();
  console.log('Tillgängliga skärmar:');
  displays.forEach((d, i) => {
    console.log(`  [${i}] ${d.size.width}x${d.size.height} @ (${d.bounds.x},${d.bounds.y})`);
  });
  const target = displays.find(d => d.size.width === 450 && d.size.height === 1920);
  if (target) { console.log('✓ Hittade 450×1920-skärm'); return target; }
  if (displays.length > 1) { console.log('↩ Sekundär skärm som fallback'); return displays[1]; }
  return null;
}

function createWindow() {
  const targetDisplay = findDashboardDisplay();
  const windowConfig = {
    width: 450, height: 1920,
    frame: false, resizable: false,
    alwaysOnTop: false,
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
  createWindow();
  // Förhindra att skärmen går i sleep mode
  powerBlockerId = powerSaveBlocker.start('prevent-display-sleep');
  console.log('✓ Display sleep mode blockerad');
});
app.on('window-all-closed', () => {
  if (powerBlockerId !== null) {
    powerSaveBlocker.stop(powerBlockerId);
    console.log('✓ Display sleep mode blockering slutad');
  }
  if (process.platform !== 'darwin') app.quit();
});

// ── Hämta data från LibreHardwareMonitor webb-API ─────────────────
// LibreHardwareMonitor kör en lokal server på http://localhost:8085
async function fetchLHM() {
  try {
    const response = await fetch('http://localhost:8085/data.json', {
      signal: AbortSignal.timeout(800)
    });
    const json = await response.json();
    return parseLHM(json);
  } catch(e) {
    return null; // LHM inte igång
  }
}

// Traversera LHM:s trädstruktur rekursivt
// parent = närmaste förälders text
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

// Parsa LHM-värden: "56,0 °C" → 56.0, "8,0 %" → 8.0, "3540,0 MB" → 3540.0
function parseVal(str) {
  if (!str || str.trim() === '') return null;
  const clean = str.replace(',', '.').replace(/[^\d.-]/g, '');
  const v = parseFloat(clean);
  return isNaN(v) ? null : v;
}

// Hitta sensor under en specifik parent-nod, matchat på text
function findUnder(nodes, parentName, textMatch) {
  const match = nodes.find(n =>
    n.parent.toLowerCase().includes(parentName.toLowerCase()) &&
    n.text.toLowerCase().includes(textMatch.toLowerCase())
  );
  return match ? parseVal(match.value) : null;
}

// Hitta N:te match (för sensorer med samma namn, t.ex. "GPU Core" finns flera gånger)
function findNth(nodes, parentName, textMatch, nth = 0) {
  const matches = nodes.filter(n =>
    n.parent.toLowerCase().includes(parentName.toLowerCase()) &&
    n.text.toLowerCase().includes(textMatch.toLowerCase())
  );
  return matches[nth] ? parseVal(matches[nth].value) : null;
}

function parseLHM(data) {
  const nodes = flattenLHM(data);

  // ── Hitta RX 9070 XT:s sensorer ─────────────────────────────────
  // LHM har två GPU-enheter (iGPU + dGPU) med identiska parent-namn.
  // Vi identifierar RX 9070 XT via att dess "GPU Memory Total" = 16304 MB.
  // Alla sensorer för dGPU ligger i ett sammanhängande block i nodes[].
  // Strategi: hitta index för dGPU:s "GPU Memory Total" och leta bakåt/framåt.

  const vramTotalIdx = nodes.findIndex(n =>
    n.text === 'GPU Memory Total' && parseVal(n.value) > 10000
  );

  // Hämta ett fönster av noder runt dGPU:s memory-nod (±30 noder)
  const gpuBlock = vramTotalIdx >= 0
    ? nodes.slice(Math.max(0, vramTotalIdx - 25), vramTotalIdx + 5)
    : [];

  // Helper: hitta värde i gpuBlock baserat på parent + text
  function gpuFind(parentMatch, textMatch) {
    const n = gpuBlock.find(n =>
      n.parent === parentMatch && n.text === textMatch
    );
    return n ? parseVal(n.value) : null;
  }

  // Hämta GPU-sensorer från blocket
  const gpuTemp     = gpuFind('Temperatures', 'GPU Core');
  const gpuHotspot  = gpuFind('Temperatures', 'GPU Hot Spot');
  const gpuMemTemp  = gpuFind('Temperatures', 'GPU Memory');
  const gpuLoad     = gpuFind('Load', 'GPU Core');
  const gpuPower    = gpuFind('Powers', 'GPU Package');
  const gpuFreq     = gpuFind('Clocks', 'GPU Core');
  const gpuFan      = gpuFind('Fans', 'GPU Fan');
  const vramUsed    = gpuFind('Data', 'GPU Memory Used');
  const vramTotal   = gpuFind('Data', 'GPU Memory Total');

  // ── CPU ──────────────────────────────────────────────────────────
  const cpuTempNode  = nodes.find(n => n.text === 'Core (Tctl/Tdie)');
  const cpuPowerNode = nodes.find(n => n.parent === 'Powers' && n.text === 'Package');
  const cpuTemp  = cpuTempNode  ? parseVal(cpuTempNode.value)  : null;
  const cpuPower = cpuPowerNode ? parseVal(cpuPowerNode.value) : null;

  // ── DISK — hämta från Throughput-noder ───────────────────────────
  // Första Throughput Read Rate = NVMe 1 (C:), andra = NVMe 2 (D:)
  const readNodes  = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Read Rate');
  const writeNodes = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Write Rate');
  // Konvertera "54,9 MB/s" eller "614,2 KB/s" till MB/s
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

  // ── NÄTVERK — hämta från Throughput Upload/Download ──────────────
  // Välj det gränssnitt med mest trafik
  const uploadNodes   = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Upload Speed');
  const downloadNodes = nodes.filter(n => n.parent === 'Throughput' && n.text === 'Download Speed');
  let bestUp = 0, bestDown = 0;
  uploadNodes.forEach((n, i) => {
    const up   = toMBs(n.value) * 8; // MB/s → Mbps
    const down = downloadNodes[i] ? toMBs(downloadNodes[i].value) * 8 : 0;
    if (up + down > bestUp + bestDown) { bestUp = up; bestDown = down; }
  });

  // ── NVMe-temp ─────────────────────────────────────────────────────
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

// ── IPC: hämta alla stats ─────────────────────────────────────────
ipcMain.handle('get-stats', async () => {
  try {
    const [cpuLoad, cpuCurrentSpeed, mem, networkStats, fsSize, lhm] = await Promise.all([
      si.currentLoad(),
      si.cpuCurrentSpeed(),
      si.mem(),
      si.networkStats(),
      si.fsSize(),
      fetchLHM()
    ]);

    // Disk — använd LHM:s Throughput-data om tillgänglig, annars fsStats
    let diskRead = lhm?.diskRead ?? 0;
    let diskWrite = lhm?.diskWrite ?? 0;
    if (!lhm) {
      try {
        const fs = await si.fsStats();
        if (fs) { diskRead = (fs.rx_sec || 0) / 1e6; diskWrite = (fs.wx_sec || 0) / 1e6; }
      } catch(e) {}
    }

    // Nätverk — använd LHM om tillgänglig, annars systeminformation
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
        free:  mem.free
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
    console.error('Stats-fel:', err);
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