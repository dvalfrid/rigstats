// Dashboard runtime orchestrator.
// Responsibilities:
// - Poll backend stats on an interval.
// - Validate and apply payloads to panel modules.
// - Protect UI stability with anti-overlap and last-known-good fallback logic.

import { IS_DESKTOP, backend } from './environment.js';
import { startClock, setUptimeFromSeconds } from './clock.js';
import { createHistory, pushHistory, drawSpark } from './spark.js';
import { updateRigName, updateCpuModel, updateGpuModel, updateRigLogo } from './systemInfo.js';
import { normalizeRigBrand } from './vendorBranding.js';
import { initCpuPanel, updateCpuPanel } from './panels/cpu.js';
import { updateGpuPanel } from './panels/gpu.js';
import { updateRamPanel } from './panels/ram.js';
import { updateNetworkPanel } from './panels/network.js';
import { updateDiskPanel } from './panels/disk.js';
import { simulateStats } from './simulator.js';

// Route uncaught JS errors and unhandled promise rejections to the backend
// debug log so they appear in the Status dialog without needing DevTools.
function logRendererError(message) {
  if (IS_DESKTOP) {
    backend.invoke('log-frontend-error', { message }).catch(() => {});
  }
}

window.addEventListener('error', (event) => {
  const msg = `${event.message} (${event.filename}:${event.lineno})`;
  logRendererError(msg);
});

window.addEventListener('unhandledrejection', (event) => {
  const reason = event.reason instanceof Error
    ? `${event.reason.message}`
    : String(event.reason ?? 'unhandled rejection');
  logRendererError(reason);
});

const BRAND_PREVIEW_ORDER = ['rog', 'msi', 'alienware', 'razer', 'legion', 'omen', 'predator', 'aorus', 'gigabyte'];
const BRAND_PREVIEW_ENABLED_KEY = 'rigstats.brandPreviewEnabled';
const BRAND_PREVIEW_BRAND_KEY = 'rigstats.brandPreviewBrand';

let detectedBrand = 'other';
let detectedCpuModel = '';
let brandPreviewEnabled = false;

const PROFILE_SIZE = {
  'portrait-xl': { width: 450, height: 1920 },
  'portrait-slim': { width: 480, height: 1920 },
  'portrait-hd': { width: 720, height: 1280 },
  'portrait-wxga': { width: 800, height: 1280 }
};

const PANEL_KEYS = ['header', 'clock', 'cpu', 'gpu', 'ram', 'net', 'disk'];

function normalizeVisiblePanels(value) {
  const list = Array.isArray(value) ? value : [];
  const normalized = list
    .map((v) => String(v).trim().toLowerCase())
    .filter((v, idx, arr) => v && PANEL_KEYS.includes(v) && arr.indexOf(v) === idx);
  return normalized.length > 0 ? normalized : [...PANEL_KEYS];
}

function applyVisiblePanels(visiblePanels) {
  const allowed = new Set(normalizeVisiblePanels(visiblePanels));
  document.querySelectorAll('.panel[data-panel]').forEach((panel) => {
    const key = panel.getAttribute('data-panel');
    panel.style.display = allowed.has(key) ? '' : 'none';
  });
}

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

function getStorageValue(key) {
  try {
    return localStorage.getItem(key);
  } catch (_e) {
    return null;
  }
}

function setStorageValue(key, value) {
  try {
    localStorage.setItem(key, value);
  } catch (_e) {}
}

function isBrandPreviewEnabled() {
  return getStorageValue(BRAND_PREVIEW_ENABLED_KEY) === '1';
}

function setBrandPreviewEnabled(enabled) {
  brandPreviewEnabled = !!enabled;
  setStorageValue(BRAND_PREVIEW_ENABLED_KEY, brandPreviewEnabled ? '1' : '0');
  renderBrandState();
}

function getPreviewBrand() {
  const forced = normalizeRigBrand(getStorageValue(BRAND_PREVIEW_BRAND_KEY));
  if (forced) return forced;
  const fromDetected = normalizeRigBrand(detectedBrand);
  return fromDetected || BRAND_PREVIEW_ORDER[0];
}

function setPreviewBrand(brand) {
  const normalized = normalizeRigBrand(brand);
  if (!normalized) return null;
  setStorageValue(BRAND_PREVIEW_BRAND_KEY, normalized);
  renderBrandState();
  return normalized;
}

function stepPreviewBrand(delta) {
  const current = getPreviewBrand();
  const currentIndex = BRAND_PREVIEW_ORDER.indexOf(current);
  const start = currentIndex >= 0 ? currentIndex : 0;
  const nextIndex = (start + delta + BRAND_PREVIEW_ORDER.length) % BRAND_PREVIEW_ORDER.length;
  return setPreviewBrand(BRAND_PREVIEW_ORDER[nextIndex]);
}

function applyBrand(brand) {
  updateRigLogo(brand, detectedCpuModel);
}

function updateBrandPreviewIndicator(text) {
  const badge = document.getElementById('rigBrandPreview');
  if (!badge) return;

  if (!text) {
    badge.textContent = '';
    badge.style.display = 'none';
    return;
  }

  badge.textContent = text;
  badge.style.display = '';
}

function renderBrandState() {
  if (brandPreviewEnabled) {
    const brand = getPreviewBrand();
    applyBrand(brand);
    updateBrandPreviewIndicator(`PREVIEW ${brand.toUpperCase()}  (Ctrl+Alt+N/P)`);
    return;
  }

  applyBrand(detectedBrand);
  updateBrandPreviewIndicator('');
}

function setupBrandPreviewControls() {
  window.rigBrandPreview = {
    list: () => [...BRAND_PREVIEW_ORDER],
    current: () => ({
      enabled: brandPreviewEnabled,
      brand: brandPreviewEnabled ? getPreviewBrand() : normalizeRigBrand(detectedBrand) || 'other'
    }),
    enable: () => setBrandPreviewEnabled(true),
    disable: () => setBrandPreviewEnabled(false),
    toggle: () => setBrandPreviewEnabled(!brandPreviewEnabled),
    next: () => {
      setBrandPreviewEnabled(true);
      return stepPreviewBrand(1);
    },
    prev: () => {
      setBrandPreviewEnabled(true);
      return stepPreviewBrand(-1);
    },
    set: (brand) => {
      setBrandPreviewEnabled(true);
      return setPreviewBrand(brand);
    }
  };

  window.addEventListener('keydown', (event) => {
    if (!event.ctrlKey || !event.altKey) return;
    const key = event.key.toLowerCase();

    if (key === 'b') {
      event.preventDefault();
      setBrandPreviewEnabled(!brandPreviewEnabled);
      return;
    }

    if (key === 'n') {
      event.preventDefault();
      setBrandPreviewEnabled(true);
      stepPreviewBrand(1);
      return;
    }

    if (key === 'p') {
      event.preventDefault();
      setBrandPreviewEnabled(true);
      stepPreviewBrand(-1);
    }
  });
}

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
  setUptimeFromSeconds(stats.systemUptimeSecs);

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
      logRendererError(`get-stats failed: ${e?.message ?? e}`);
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
  setupBrandPreviewControls();
  initCpuPanel();
  startClock();
  setUptimeFromSeconds(0);
  brandPreviewEnabled = isBrandPreviewEnabled();

  if (IS_DESKTOP) {
    backend.invoke('get-settings').then((s) => {
      applyOpacity(s.opacity);
      applyModelName(s.modelName);
      applyProfile(s.dashboardProfile);
      applyVisiblePanels(s.visiblePanels);
    });
    Promise.all([
      backend.invoke('get-system-brand').catch(() => 'other'),
      backend.invoke('get-cpu-info').catch(() => '')
    ]).then(([brand, cpu]) => {
      detectedBrand = brand || 'other';
      detectedCpuModel = cpu || '';
      renderBrandState();
    });
    Promise.all([
      backend.on('apply-opacity', (_event, value) => applyOpacity(value)),
      backend.on('apply-model-name', (_event, name) => applyModelName(name)),
      backend.on('apply-profile', (_event, profile) => applyProfile(profile)),
      backend.on('apply-visible-panels', (_event, panels) => applyVisiblePanels(panels)),
    ]).then((unlisteners) => {
      window.addEventListener('beforeunload', () => unlisteners.forEach((fn) => fn()));
    });
  } else {
    applyVisiblePanels(PANEL_KEYS);
    renderBrandState();
  }

  updateRigName();
  updateCpuModel();
  updateGpuModel();

  tick();
  setInterval(tick, 1000);
}

start();
