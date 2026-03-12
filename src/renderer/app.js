import { IS_ELECTRON, ipcRenderer } from './environment.js';
import { startClock, startUptime } from './clock.js';
import { createHistory, pushHistory, drawSpark } from './spark.js';
import { updateRigName, updateCpuModel, updateGpuModel } from './systemInfo.js';
import { initCpuPanel, updateCpuPanel } from './panels/cpu.js';
import { updateGpuPanel } from './panels/gpu.js';
import { updateRamPanel } from './panels/ram.js';
import { updateNetworkPanel } from './panels/network.js';
import { updateDiskPanel } from './panels/disk.js';
import { simulateStats } from './simulator.js';

const history = createHistory(80);

function applyStats(stats) {
  if (!stats) return;

  updateCpuPanel(stats.cpu, history, pushHistory);
  updateGpuPanel(stats.gpu, history, pushHistory);
  updateRamPanel(stats.ram, history, pushHistory);
  updateNetworkPanel(stats, history, pushHistory);
  updateDiskPanel(stats.disk, history, pushHistory);

  drawSpark('cpuSpark', history.cpu, '#00c8ff');
  drawSpark('gpuSpark', history.gpu, '#ff3a1f');
  drawSpark('ramSpark', history.ram, '#ffb300');
  drawSpark('netSpark', history.net, '#39ff88');
  drawSpark('diskSpark', history.disk, '#bf7fff');
}

async function tick() {
  if (IS_ELECTRON) {
    try {
      const stats = await ipcRenderer.invoke('get-stats');
      applyStats(stats);
      return;
    } catch (e) {
      console.error('IPC error:', e);
    }
  }

  applyStats(simulateStats());
}

function start() {
  initCpuPanel();
  startClock();
  startUptime();

  updateRigName();
  updateCpuModel();
  updateGpuModel();

  tick();
  setInterval(tick, 1000);
}

start();
