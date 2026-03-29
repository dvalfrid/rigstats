// Dashboard runtime orchestrator.
// Responsibilities:
// - Poll backend stats on an interval.
// - Validate and apply payloads to panel modules.
// - Protect UI stability with anti-overlap and last-known-good fallback logic.

import { IS_DESKTOP, backend } from './environment.js';
import { startClock, setUptimeFromSeconds } from './clock.js';
import { createHistory, pushHistory, drawSpark, drawDoubleSpark } from './spark.js';
import { updateRigName, updateCpuModel, updateGpuModel, updateRigLogo } from './systemInfo.js';
import { normalizeRigBrand } from './vendorBranding.js';
import { initCpuPanel, updateCpuPanel } from './panels/cpu.js';
import { updateGpuPanel } from './panels/gpu.js';
import { updateRamPanel } from './panels/ram.js';
import { updateNetworkPanel } from './panels/network.js';
import { updateDiskPanel } from './panels/disk.js';
import { updateMotherboardPanel } from './panels/motherboard.js';
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
let thresholds = {};

const PROFILE_SIZE = {
  'portrait-xl': { width: 450, height: 1920 },
  'portrait-slim': { width: 480, height: 1920 },
  'portrait-fhd-side': { width: 253, height: 1080 },
  'portrait-qhd-side': { width: 338, height: 1440 },
  'portrait-4k-side': { width: 506, height: 2160 },
  'portrait-hd': { width: 720, height: 1280 },
  'portrait-wxga': { width: 800, height: 1280 },
  'portrait-fhd': { width: 1080, height: 1920 },
  'portrait-wuxga': { width: 1200, height: 1920 },
  'portrait-qhd': { width: 1440, height: 2560 },
  'portrait-hdplus': { width: 768, height: 1366 },
  'portrait-900x1600': { width: 900, height: 1600 },
  'portrait-1050x1680': { width: 1050, height: 1680 },
  'portrait-1600x2560': { width: 1600, height: 2560 },
  'portrait-4k': { width: 2160, height: 3840 },
};

const BASE_PROFILE_HEIGHT = 1920;
const BASE_PROFILE_WIDTH = 450;
const BASE_ROW_HEIGHTS = [196, 148, 420, 320, 315, 260, 295, 260];

function setPxVar(style, name, value) {
  style.setProperty(name, `${Math.max(1, Math.round(value))}px`);
}

function applyProfileMetrics(profile) {
  const root = document.documentElement.style;
  const heightScale = profile.height / BASE_PROFILE_HEIGHT;
  const widthScale = profile.width / BASE_PROFILE_WIDTH;
  const contentScale = Math.max(0.50, Math.min(1.35, Math.min(heightScale, widthScale)));
  const gap = Math.max(1, Math.round(heightScale));
  const availableHeight = profile.height - (gap * (BASE_ROW_HEIGHTS.length - 1));

  let remaining = availableHeight;
  BASE_ROW_HEIGHTS.forEach((baseHeight, index) => {
    const rowName = ['--row-header', '--row-clock', '--row-cpu', '--row-gpu', '--row-ram', '--row-net', '--row-disk', '--row-mb'][index];
    // Index 6 (disk) absorbs rounding remainder so the 7 default panels fill the
    // screen exactly. Panels added beyond the original 7 use their scaled height directly.
    const raw = index === 6 ? remaining : Math.round(baseHeight * heightScale);
    const applied = Math.max(1, raw);
    remaining -= applied;
    root.setProperty(rowName, `${applied}px`);
  });

  setPxVar(root, '--dashboard-w', profile.width);
  setPxVar(root, '--dashboard-h', profile.height);
  setPxVar(root, '--panel-gap', gap);
  setPxVar(root, '--panel-pad-y', 22 * contentScale);
  setPxVar(root, '--panel-pad-x', 24 * Math.min(1.2, Math.max(0.55, widthScale)));
  setPxVar(root, '--brand-mark-size', 165 * contentScale);
  setPxVar(root, '--rig-name-size', 44 * contentScale);
  setPxVar(root, '--model-name-size', 28 * contentScale);
  setPxVar(root, '--clock-time-size', 70 * contentScale);
  setPxVar(root, '--clock-day-size', 18 * contentScale);
  setPxVar(root, '--big-num-size', 78 * contentScale);
  setPxVar(root, '--ram-big-num-size', 64 * contentScale);
  setPxVar(root, '--panel-model-size', 10 * contentScale);
  setPxVar(root, '--ring-size', 100 * contentScale);
  setPxVar(root, '--cpu-cores-max-h', 150 * heightScale);
  setPxVar(root, '--badge-size', 96 * contentScale);
  setPxVar(root, '--spark-h', 48 * heightScale);
  setPxVar(root, '--big-unit-size', 20 * contentScale);
  setPxVar(root, '--net-val-size', 28 * contentScale);
  setPxVar(root, '--disk-val-size', 24 * contentScale);
  setPxVar(root, '--font-ui', 14 * contentScale);
  setPxVar(root, '--font-sub', 12 * contentScale);
  setPxVar(root, '--gap-inner', 12 * contentScale);
  setPxVar(root, '--gap-inner-sm', 10 * contentScale);
}

const PANEL_KEYS = ['header', 'clock', 'cpu', 'gpu', 'ram', 'net', 'disk', 'motherboard'];

let currentProfile = PROFILE_SIZE['portrait-xl'];

function normalizeVisiblePanels(value) {
  const list = Array.isArray(value) ? value : [];
  const normalized = list
    .map((v) => String(v).trim().toLowerCase())
    .filter((v, idx, arr) => v && PANEL_KEYS.includes(v) && arr.indexOf(v) === idx);
  return normalized.length > 0 ? normalized : [...PANEL_KEYS];
}

function applyVisiblePanels(visiblePanels) {
  const ordered = normalizeVisiblePanels(visiblePanels);
  const dashboard = document.querySelector('.dashboard');
  const panelEls = {};
  document.querySelectorAll('.panel[data-panel]').forEach((p) => {
    panelEls[p.dataset.panel] = p;
    p.style.display = 'none';
  });
  ordered.forEach((key) => {
    if (panelEls[key]) {
      panelEls[key].style.display = '';
      dashboard.appendChild(panelEls[key]);
    }
  });

  // Shrink viewport and window to the height of the visible panels.
  // Mirror applyProfileMetrics exactly so heights match the CSS variables.
  const heightScale = currentProfile.height / BASE_PROFILE_HEIGHT;
  const gap = Math.max(1, Math.round(heightScale));
  const availableH = currentProfile.height - gap * (PANEL_KEYS.length - 1);
  const panelH = {};
  let rem = availableH;
  PANEL_KEYS.forEach((key, i) => {
    const h = i === 6 ? rem : Math.round(BASE_ROW_HEIGHTS[i] * heightScale);
    panelH[key] = Math.max(1, h);
    rem -= panelH[key];
  });
  let totalH = 0;
  ordered.forEach((key, i) => {
    totalH += (panelH[key] ?? 0) + (i > 0 ? gap : 0);
  });
  const root = document.documentElement.style;
  root.setProperty('--viewport-h', `${totalH}px`);
  root.setProperty('--dashboard-h', `${totalH}px`);

  if (IS_DESKTOP) {
    backend.invoke('set-main-height', { width: currentProfile.width, height: totalH }).catch(() => {});
  }
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
  currentProfile = profile;

  const root = document.documentElement;
  root.dataset.profile = key;
  root.style.setProperty('--viewport-w', `${profile.width}px`);
  root.style.setProperty('--viewport-h', `${profile.height}px`);
  applyProfileMetrics(profile);
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
      brand: brandPreviewEnabled ? getPreviewBrand() : normalizeRigBrand(detectedBrand) || 'other',
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
    },
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

function applyThresholds(s) {
  thresholds = {
    cpu:  { warn: s.warningCpuTemp  ?? null, crit: s.criticalCpuTemp  ?? null },
    gpu:  { warn: s.warningGpuTemp  ?? null, crit: s.criticalGpuTemp  ?? null },
    ram:  { warn: s.warningRamTemp  ?? null, crit: s.criticalRamTemp  ?? null },
    disk: { warn: s.warningDiskTemp ?? null, crit: s.criticalDiskTemp ?? null },
  };
}

function applyStats(stats) {
  if (!stats) return;

  updateCpuPanel(stats.cpu, history, pushHistory, thresholds.cpu);
  updateGpuPanel(stats.gpu, history, pushHistory, thresholds.gpu);
  updateRamPanel(stats.ram, history, pushHistory, thresholds.ram);
  updateNetworkPanel(stats, history, pushHistory);
  updateDiskPanel(stats.disk, history, pushHistory, thresholds.disk);
  updateMotherboardPanel(stats.motherboard ?? {});
  setUptimeFromSeconds(stats.systemUptimeSecs);

  drawSpark('cpuSpark', history.cpu, '#00c8ff');
  drawSpark('gpuSpark', history.gpu, '#ff3a1f');
  drawSpark('ramSpark', history.ram, '#ffb300');
  drawDoubleSpark('netSpark', history.netDown, '#00c8ff', history.netUp, '#39ff88');
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
      applyThresholds(s);
    });
    Promise.all([
      backend.invoke('get-system-brand').catch(() => 'other'),
      backend.invoke('get-cpu-info').catch(() => ''),
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
      backend.on('apply-thresholds', (_event, t) => applyThresholds(t)),
      backend.on('update-available', (_event, version) => {
        const badge = document.getElementById('updateBadge');
        if (badge) {
          badge.textContent = `↑ UPDATE  v${version}`;
          badge.style.display = '';
          badge.addEventListener('click', () => backend.invoke('open-updater-window').catch(() => {}));
        }
      }),
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
