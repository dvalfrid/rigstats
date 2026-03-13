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

function applyOpacity(value) {
  const parsed = parseFloat(value);
  const v = Math.min(1, Math.max(0, isNaN(parsed) ? 0.55 : parsed));
  const root = document.documentElement.style;
  root.setProperty('--panel', `rgba(11,13,18,${v.toFixed(2)})`);
  root.setProperty('--border', `rgba(22,28,42,${Math.max(0, v - 0.2).toFixed(2)})`);
}

function applyModelName(name) {
  const el = document.getElementById('modelName');
  if (el && name) el.textContent = name;
}

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

  if (IS_ELECTRON) {
    ipcRenderer.invoke('get-settings').then((s) => {
      applyOpacity(s.opacity);
      applyModelName(s.modelName);
    });
    ipcRenderer.on('apply-opacity', (_event, value) => applyOpacity(value));
    ipcRenderer.on('apply-model-name', (_event, name) => applyModelName(name));
  }

  updateRigName();
  updateCpuModel();
  updateGpuModel();

  tick();
  setInterval(tick, 1000);
}

start();
