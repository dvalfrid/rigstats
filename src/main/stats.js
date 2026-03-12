const si = require('systeminformation');

async function fetchLHM() {
  try {
    const response = await fetch('http://localhost:8085/data.json', {
      signal: AbortSignal.timeout(800)
    });
    const json = await response.json();
    return parseLHM(json);
  } catch (e) {
    return null;
  }
}

function flattenLHM(node, results = [], parent = '') {
  const myText = node.Text || node.text || '';
  const myVal = node.Value || node.value || '';
  if (myVal !== '' && myVal !== 'Value') {
    results.push({ text: myText, value: myVal, parent });
  }

  const children = node.Children || node.children || [];
  children.forEach((c) => flattenLHM(c, results, myText || parent));
  return results;
}

function parseVal(str) {
  if (!str || str.trim() === '') return null;
  const clean = str.replace(',', '.').replace(/[^\d.-]/g, '');
  const v = parseFloat(clean);
  return Number.isNaN(v) ? null : v;
}

function parseLHM(data) {
  const nodes = flattenLHM(data);

  const vramTotalIdx = nodes.findIndex((n) =>
    n.text === 'GPU Memory Total' && parseVal(n.value) > 10000
  );

  const gpuBlock = vramTotalIdx >= 0
    ? nodes.slice(Math.max(0, vramTotalIdx - 25), vramTotalIdx + 5)
    : [];

  function gpuFind(parentMatch, textMatch) {
    const n = gpuBlock.find((node) => node.parent === parentMatch && node.text === textMatch);
    return n ? parseVal(n.value) : null;
  }

  const gpuTemp = gpuFind('Temperatures', 'GPU Core');
  const gpuHotspot = gpuFind('Temperatures', 'GPU Hot Spot');
  const gpuMemTemp = gpuFind('Temperatures', 'GPU Memory');
  const gpuLoad = gpuFind('Load', 'GPU Core');
  const gpuPower = gpuFind('Powers', 'GPU Package');
  const gpuFreq = gpuFind('Clocks', 'GPU Core');
  const gpuFan = gpuFind('Fans', 'GPU Fan');
  const vramUsed = gpuFind('Data', 'GPU Memory Used');
  const vramTotal = gpuFind('Data', 'GPU Memory Total');

  const cpuTempNode = nodes.find((n) => n.text === 'Core (Tctl/Tdie)');
  const cpuPowerNode = nodes.find((n) => n.parent === 'Powers' && n.text === 'Package');
  const cpuTemp = cpuTempNode ? parseVal(cpuTempNode.value) : null;
  const cpuPower = cpuPowerNode ? parseVal(cpuPowerNode.value) : null;

  const readNodes = nodes.filter((n) => n.parent === 'Throughput' && n.text === 'Read Rate');
  const writeNodes = nodes.filter((n) => n.parent === 'Throughput' && n.text === 'Write Rate');

  function toMBs(str) {
    if (!str) return 0;
    const v = parseVal(str);
    if (str.includes('KB')) return v / 1024;
    if (str.includes('GB')) return v * 1024;
    return v;
  }

  const disk1Read = readNodes[0] ? toMBs(readNodes[0].value) : 0;
  const disk1Write = writeNodes[0] ? toMBs(writeNodes[0].value) : 0;
  const disk2Read = readNodes[1] ? toMBs(readNodes[1].value) : 0;
  const disk2Write = writeNodes[1] ? toMBs(writeNodes[1].value) : 0;

  const uploadNodes = nodes.filter((n) => n.parent === 'Throughput' && n.text === 'Upload Speed');
  const downloadNodes = nodes.filter((n) => n.parent === 'Throughput' && n.text === 'Download Speed');

  let bestUp = 0;
  let bestDown = 0;
  uploadNodes.forEach((n, i) => {
    const up = toMBs(n.value) * 8;
    const down = downloadNodes[i] ? toMBs(downloadNodes[i].value) * 8 : 0;
    if (up + down > bestUp + bestDown) {
      bestUp = up;
      bestDown = down;
    }
  });

  const nvmeTempNodes = nodes.filter((n) => n.text === 'Composite Temperature');
  const nvme1Temp = nvmeTempNodes[0] ? parseVal(nvmeTempNodes[0].value) : null;
  const nvme2Temp = nvmeTempNodes[1] ? parseVal(nvmeTempNodes[1].value) : null;

  return {
    gpuLoad,
    gpuTemp,
    gpuHotspot,
    gpuMemTemp,
    gpuFreq,
    gpuPower,
    gpuFan,
    vramUsed,
    vramTotal,
    cpuTemp,
    cpuPower,
    diskRead: disk1Read + disk2Read,
    diskWrite: disk1Write + disk2Write,
    disk1Read,
    disk1Write,
    disk2Read,
    disk2Write,
    netUp: bestUp,
    netDown: bestDown,
    nvme1Temp,
    nvme2Temp
  };
}

function buildRamSpec(memLayout) {
  const dimms = (memLayout || []).filter((m) => (m.size || 0) > 0);
  const ramTypes = [...new Set(dimms.map((m) => m.type).filter(Boolean))];
  const speedCandidates = dimms
    .map((m) => Number(m.clockSpeed || m.frequency || m.configuredClockSpeed || 0))
    .filter((v) => Number.isFinite(v) && v > 0);
  const maxSpeed = speedCandidates.length ? Math.max(...speedCandidates) : null;

  let ramSpec = ramTypes.length ? ramTypes.join('/') : 'RAM';
  if (maxSpeed) ramSpec += ` ${Math.round(maxSpeed)} MT/s`;
  if (dimms.length) ramSpec += ` (${dimms.length} DIMMs)`;

  return ramSpec;
}

async function getStats() {
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

    let diskRead = lhm?.diskRead ?? 0;
    let diskWrite = lhm?.diskWrite ?? 0;
    if (!lhm) {
      try {
        const fs = await si.fsStats();
        if (fs) {
          diskRead = (fs.rx_sec || 0) / 1e6;
          diskWrite = (fs.wx_sec || 0) / 1e6;
        }
      } catch (e) {}
    }

    let netUp = lhm?.netUp ?? 0;
    let netDown = lhm?.netDown ?? 0;
    let netIface = '—';

    if (!lhm) {
      const activeNet = (networkStats || []).sort((a, b) =>
        (b.rx_sec + b.tx_sec) - (a.rx_sec + a.tx_sec)
      )[0] || {};
      netUp = (activeNet.tx_sec || 0) * 8 / 1e6;
      netDown = (activeNet.rx_sec || 0) * 8 / 1e6;
      netIface = activeNet.iface || '—';
    }

    const drives = (fsSize || []).filter((f) => f.size > 1e9);
    const vramUsedMB = lhm?.vramUsed ?? null;
    const vramTotalMB = lhm?.vramTotal ?? 16384;

    return {
      cpu: {
        load: Math.round(cpuLoad.currentLoad),
        cores: cpuLoad.cpus.map((c) => Math.round(c.load)),
        temp: lhm?.cpuTemp ?? null,
        freq: cpuCurrentSpeed.avg || 0,
        power: lhm?.cpuPower ?? null
      },
      gpu: {
        load: lhm?.gpuLoad ?? null,
        temp: lhm?.gpuTemp ?? null,
        hotspot: lhm?.gpuHotspot ?? null,
        freq: lhm?.gpuFreq ?? null,
        vramUsed: vramUsedMB,
        vramTotal: vramTotalMB,
        fanSpeed: lhm?.gpuFan ?? null,
        power: lhm?.gpuPower ?? null
      },
      ram: {
        total: mem.total,
        used: mem.used,
        free: mem.free,
        spec: buildRamSpec(memLayout)
      },
      net: {
        up: netUp,
        down: netDown,
        iface: netIface
      },
      disk: {
        read: diskRead,
        write: diskWrite,
        drives: drives.map((d) => ({
          fs: d.fs,
          size: d.size,
          used: d.used,
          pct: Math.round(d.use || 0)
        }))
      },
      lhmConnected: lhm !== null
    };
  } catch (err) {
    console.error('Stats error:', err);
    return null;
  }
}

module.exports = {
  getStats
};
