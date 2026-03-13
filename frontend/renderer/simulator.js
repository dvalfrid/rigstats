// Synthetic stats generator for non-desktop/browser preview mode.
// Keeps values in realistic ranges so panel layout and animations are testable.

let cpuTrend = 35;
let gpuTrend = 60;
let ramTrend = 55;
const simulatorStartMs = Date.now();

function simulateStats() {
  cpuTrend = Math.max(5, Math.min(97, cpuTrend + (Math.random() - 0.47) * 7));
  gpuTrend = Math.max(10, Math.min(99, gpuTrend + (Math.random() - 0.46) * 5));
  ramTrend = Math.max(35, Math.min(88, ramTrend + (Math.random() - 0.5) * 3));

  const cpu = Math.round(cpuTrend);
  const gpu = Math.round(gpuTrend);
  const ram = Math.round(ramTrend);
  const totalRam = 32 * 1073741824;

  return {
    cpu: {
      load: cpu,
      cores: Array(8).fill(0).map(() => Math.max(2, Math.min(99, cpu + (Math.random() - 0.5) * 35)) | 0),
      temp: 48 + cpu * 0.38,
      freq: 3.2 + cpu * 0.02,
      power: 35 + cpu * 1.4
    },
    gpu: {
      load: gpu,
      temp: 58 + gpu * 0.22,
      hotspot: 70 + gpu * 0.2,
      vramUsed: gpu * 140 + 1000,
      vramTotal: 16384,
      fanSpeed: gpu > 45 ? (900 + gpu * 22) | 0 : 0,
      power: 30 + gpu * 3.3,
      freq: 1800 + gpu * 8
    },
    ram: {
      total: totalRam,
      used: totalRam * ram / 100,
      free: totalRam * (1 - ram / 100),
      spec: 'DDR5 6000 MT/s (2 DIMMs)',
      details: '2x16 GB | Kingston | KF560C36-16'
    },
    net: {
      up: Math.random() * 40e6 + 2e6,
      down: Math.random() * 100e6 + 5e6,
      iface: 'Ethernet',
      pingMs: 5 + Math.random() * 25
    },
    disk: {
      read: Math.random() * 3e9,
      write: Math.random() * 1.5e9,
      drives: [
        { fs: 'C:', size: 1e12, used: 5.5e11, pct: 55 },
        { fs: 'D:', size: 4e12, used: 1.2e12, pct: 30 }
      ]
    },
    systemUptimeSecs: Math.floor((Date.now() - simulatorStartMs) / 1000),
    lhmConnected: false
  };
}

export { simulateStats };
