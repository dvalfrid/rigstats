// Dashboard runtime orchestrator.
// Responsibilities:
// - Poll backend stats on an interval.
// - Validate and apply payloads to panel modules.
// - Protect UI stability with anti-overlap and last-known-good fallback logic.

import { IS_DESKTOP, backend } from './environment.js';
import { startClock, startUptime } from './clock.js';
import { createHistory, pushHistory, drawSpark } from './spark.js';
import { updateRigName, updateCpuModel, updateGpuModel } from './systemInfo.js';
import { initCpuPanel, updateCpuPanel } from './panels/cpu.js';
import { updateGpuPanel } from './panels/gpu.js';
import { updateRamPanel } from './panels/ram.js';
import { updateNetworkPanel } from './panels/network.js';
import { updateDiskPanel } from './panels/disk.js';
import { simulateStats } from './simulator.js';

const PROFILE_SIZE = {
  'portrait-xl': { width: 450, height: 1920 },
  'portrait-slim': { width: 480, height: 1920 },
  'portrait-hd': { width: 720, height: 1280 },
  'portrait-wxga': { width: 800, height: 1280 }
};

function applyOpacity(value) {
  // Opacity is applied via CSS variables to keep styling centralized.
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

function applyProfile(profileName) {
  const key = PROFILE_SIZE[profileName] ? profileName : 'portrait-xl';
  const profile = PROFILE_SIZE[key];
  const scale = Math.min(profile.width / 450, profile.height / 1920);

  const root = document.documentElement;
  root.dataset.profile = key;
  root.style.setProperty('--viewport-w', `${profile.width}px`);
  root.style.setProperty('--viewport-h', `${profile.height}px`);
  root.style.setProperty('--dashboard-scale', String(scale));
}

function initWindowDrag() {
  if (!IS_DESKTOP) return;

  const header = document.querySelector('.panel-header');
  if (!header) return;

  header.addEventListener('mousedown', (event) => {
    if (event.button !== 0) return;
    backend.invoke('start-window-drag').catch((error) => {
      console.error('Failed to start window drag:', error);
    });
  });
}

const history = createHistory(80);
let isTicking = false;
let lastValidStats = null;

function isValidStatsPayload(stats) {
  // Defensive validation: reject transient empty payloads that would reset UI.
  if (!stats || !stats.cpu || !stats.ram || !stats.net || !stats.disk) return false;
  if (!Array.isArray(stats.cpu.cores) || stats.cpu.cores.length === 0) return false;
  if (!Number.isFinite(stats.ram.total) || stats.ram.total <= 0) return false;
  if (!Number.isFinite(stats.ram.used) || stats.ram.used < 0) return false;
  return true;
}

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
  // Skip if previous sample is still in flight to avoid out-of-order updates.
  if (isTicking) return;
  isTicking = true;

  if (IS_DESKTOP) {
    try {
      const stats = await backend.invoke('get-stats');
      if (isValidStatsPayload(stats)) {
        lastValidStats = stats;
        applyStats(stats);
      } else if (lastValidStats) {
        // Reuse last valid sample to avoid visual reset/blink.
        applyStats(lastValidStats);
      }
    } catch (e) {
      console.error('Backend error:', e);
      if (lastValidStats) applyStats(lastValidStats);
    } finally {
      isTicking = false;
    }

    // In desktop mode, keep last rendered values on transient backend errors.
    return;
  }

  applyStats(simulateStats());
  isTicking = false;
}

function start() {
  applyProfile('portrait-xl');
  initWindowDrag();
  initCpuPanel();
  startClock();
  startUptime();

  if (IS_DESKTOP) {
    backend.invoke('get-settings').then((s) => {
      applyOpacity(s.opacity);
      applyModelName(s.modelName);
      applyProfile(s.dashboardProfile);
    });
    backend.on('apply-opacity', (_event, value) => applyOpacity(value));
    backend.on('apply-model-name', (_event, name) => applyModelName(name));
    backend.on('apply-profile', (_event, profile) => applyProfile(profile));
  }

  updateRigName();
  updateCpuModel();
  updateGpuModel();

  tick();
  setInterval(tick, 1000);
}

start();
