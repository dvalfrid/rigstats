// Shared entry point for all floating panel windows.
// Detects which panel this window hosts from the Tauri window label,
// subscribes to the stats-broadcast app event, and drives updates.

import { IS_DESKTOP, backend } from './environment.js';
import { startClock, setUptimeFromSeconds } from './clock.js';
import { createHistory, pushHistory, drawSpark, drawDoubleSpark } from './spark.js';
import { initCpuPanel, updateCpuPanel } from './panels/cpu.js';
import { updateGpuPanel } from './panels/gpu.js';
import { updateRamPanel } from './panels/ram.js';
import { updateNetworkPanel } from './panels/network.js';
import { updateDiskPanel } from './panels/disk.js';
import { updateMotherboardPanel } from './panels/motherboard.js';
import { updateProcessPanel } from './panels/process.js';
import { updateRigName, updateCpuModel, updateGpuModel, updateRigLogo } from './systemInfo.js';
import { applyTheme } from './themes.js';

// --- Panel identity ---------------------------------------------------------

// Extract the panel key from the HTML file name — reliable across all Tauri
// versions and regardless of whether the WebviewWindow API is accessible.
// URL path is e.g. /panel-cpu.html → key = 'cpu'
const panelKey = (() => {
  const m = window.location.pathname.match(/panel-([^/.]+)\.html/);
  if (m) return m[1];
  // Fallback: try Tauri v2 WebviewWindow API paths
  const label =
    window.__TAURI__?.webviewWindow?.getCurrent?.()?.label ??
    window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.()?.label ??
    window.__TAURI__?.window?.getCurrent?.()?.label;
  return label?.replace(/^panel-/, '') ?? '';
})();

// Window reference for outerPosition() and move-event listening.
// Try Tauri v2 paths in order.
const currentWindow = (() => {
  try {
    return (
      window.__TAURI__?.webviewWindow?.getCurrent?.() ??
      window.__TAURI__?.webviewWindow?.getCurrentWebviewWindow?.() ??
      window.__TAURI__?.window?.getCurrent?.()
    );
  } catch (_e) {
    return null;
  }
})();

// --- Shared history (sparklines) --------------------------------------------

const history = createHistory(80);

// --- Profile/size scaling ---------------------------------------------------

const BASE_WIDTH = 450;
const BASE_PANEL_HEIGHT = {
  header: 196,
  clock: 148,
  cpu: 420,
  gpu: 320,
  ram: 315,
  net: 260,
  disk: 295,
  motherboard: 260,
  process: 260,
};

function setPxVar(style, name, value) {
  style.setProperty(name, `${Math.max(1, Math.round(value))}px`);
}

function applyWindowScaling(windowW, windowH) {
  const panel = document.querySelector('.panel[data-panel]');
  if (!panel) return;

  const panelH = Math.max(1, windowH);

  document.documentElement.style.width = `${Math.round(windowW)}px`;
  document.documentElement.style.height = `${Math.round(windowH)}px`;
  document.body.style.width = `${Math.round(windowW)}px`;
  document.body.style.height = `${Math.round(windowH)}px`;
  panel.style.height = `${Math.round(panelH)}px`;

  const baseH = BASE_PANEL_HEIGHT[panelKey] ?? 260;
  const widthScale = windowW / BASE_WIDTH;
  const heightScale = panelH / baseH;
  const contentScale = Math.max(0.50, Math.min(1.35, Math.min(widthScale, heightScale)));

  const root = document.documentElement.style;
  setPxVar(root, '--panel-pad-y', 22 * contentScale);
  setPxVar(root, '--panel-pad-x', 24 * Math.min(1.2, Math.max(0.55, widthScale)));
  setPxVar(root, '--big-num-size', 78 * contentScale);
  setPxVar(root, '--ram-big-num-size', 64 * contentScale);
  setPxVar(root, '--font-ui', 14 * contentScale);
  setPxVar(root, '--font-sub', 12 * contentScale);
  setPxVar(root, '--ring-size', 100 * contentScale);
  setPxVar(root, '--cpu-cores-max-h', 150 * heightScale);
  setPxVar(root, '--badge-size', 96 * contentScale);
  setPxVar(root, '--spark-h', 48 * heightScale);
  setPxVar(root, '--big-unit-size', 20 * contentScale);
  setPxVar(root, '--net-val-size', 28 * contentScale);
  setPxVar(root, '--disk-val-size', 24 * contentScale);
  setPxVar(root, '--gap-inner', 12 * contentScale);
  setPxVar(root, '--gap-inner-sm', 10 * contentScale);
  setPxVar(root, '--clock-time-size', 70 * contentScale);
  setPxVar(root, '--clock-day-size', 18 * contentScale);
  setPxVar(root, '--rig-name-size', 44 * contentScale);
  setPxVar(root, '--model-name-size', 28 * contentScale);
  setPxVar(root, '--brand-mark-size', 165 * contentScale);
}

async function syncWindowScaling() {
  try {
    if (currentWindow?.innerSize) {
      const size = await currentWindow.innerSize();
      applyWindowScaling(size.width, size.height);
      return;
    }
  } catch (_e) {
    // Fall through to DOM dimensions.
  }
  applyWindowScaling(window.innerWidth, window.innerHeight);
}

// --- Thresholds -------------------------------------------------------------

let thresholds = {};

function applyThresholds(payload) {
  thresholds = {};
  for (const [key, val] of Object.entries(payload.thresholds ?? {})) {
    thresholds[key] = { warn: val?.warn ?? null, crit: val?.crit ?? null };
  }
}

// --- Opacity ----------------------------------------------------------------

function applyOpacity(value) {
  const parsed = parseFloat(value);
  const v = Math.min(1, Math.max(0, isNaN(parsed) ? 0.55 : parsed));
  const root = document.documentElement.style;
  root.setProperty('--panel', `rgba(11,13,18,${v.toFixed(2)})`);
  root.setProperty('--border', `rgba(22,28,42,${Math.max(0, v - 0.2).toFixed(2)})`);
}

function getCssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

// --- Stats dispatch ---------------------------------------------------------

function applyStats(stats) {
  if (!stats) return;
  switch (panelKey) {
    case 'cpu':
      updateCpuPanel(stats.cpu, history, pushHistory, thresholds.cpu);
      drawSpark('cpuSpark', history.cpu, getCssVar('--accent'));
      break;
    case 'gpu':
      updateGpuPanel(stats.gpu, history, pushHistory, thresholds.gpu);
      drawSpark('gpuSpark', history.gpu, getCssVar('--amd'));
      break;
    case 'ram':
      updateRamPanel(stats.ram, history, pushHistory, thresholds.ram);
      drawSpark('ramSpark', history.ram, getCssVar('--ram'));
      break;
    case 'net':
      updateNetworkPanel(stats, history, pushHistory);
      drawDoubleSpark('netSpark', history.netDown, getCssVar('--accent'), history.netUp, getCssVar('--grn'));
      break;
    case 'disk':
      updateDiskPanel(stats.disk, history, pushHistory, thresholds.disk);
      drawDoubleSpark('diskSpark', history.diskRead, getCssVar('--pur'), history.diskWrite, getCssVar('--disk-write'));
      break;
    case 'motherboard':
      updateMotherboardPanel(stats.motherboard ?? {});
      break;
    case 'process':
      updateProcessPanel(stats.topProcesses ?? []);
      break;
    case 'header':
      setUptimeFromSeconds(stats.systemUptimeSecs);
      break;
    case 'clock':
      setUptimeFromSeconds(stats.systemUptimeSecs);
      break;
    default:
      break;
  }
}

// --- Position saving (debounced on window move) ----------------------------

let moveSaveTimer = null;

function scheduleSavePosition() {
  clearTimeout(moveSaveTimer);
  moveSaveTimer = setTimeout(async () => {
    if (!currentWindow) return;
    try {
      const pos = await currentWindow.outerPosition();
      backend.invoke('save-panel-positions', {
        positions: { [panelKey]: { x: pos.x, y: pos.y } },
      }).catch(() => {});
    } catch (_e) {
      // Position unavailable — skip silently.
    }
  }, 500);
}

// --- Context menu -----------------------------------------------------------
// --- Drag handling ---------------------------------------------------------
// Add explicit mousedown → start-window-drag IPC so dragging works reliably
// on transparent borderless windows (mirrors how the main window does it).
function initDrag() {
  let lastDragStartTs = 0;

  const tryStartDrag = (e) => {
    const button = Number.isFinite(e.button) ? e.button : 0;
    if (button !== 0) return;

    const now = Date.now();
    if (now - lastDragStartTs < 120) return;
    lastDragStartTs = now;

    // Keep interactive or scrollable regions usable.
    if (e.target.closest('#ctx-menu, button, a, input, select, textarea')) return;
    if (e.target.closest('#cpuCores, .mb-scroll')) return;
    if (e.target.id === 'updateBadge' || e.target.closest('#updateBadge')) return;
    // Don't start drag when the context menu is open.
    const menu = document.getElementById('ctx-menu');
    if (menu && menu.style.display === 'block') return;
    backend.invoke('start-window-drag').catch((error) => {
      const message = `[panel-host:${panelKey}] start-window-drag failed: ${String(error)}`;
      backend.invoke('log-frontend-error', { message }).catch(() => {});
    });
  };

  document.getElementById('dragHandle')?.addEventListener('pointerdown', tryStartDrag, true);
  document.body.addEventListener('pointerdown', tryStartDrag, true);
}

// --- Context menu -----------------------------------------------------------

function initContextMenu() {
  const handle = document.getElementById('dragHandle');
  const menu = document.getElementById('ctx-menu');
  if (!handle || !menu) return;

  handle.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    menu.style.display = 'block';
    menu.style.left = `${e.clientX}px`;
    menu.style.top = `${e.clientY}px`;
  });

  document.getElementById('ctxSettings')?.addEventListener('click', () => {
    menu.style.display = 'none';
    backend.invoke('open-settings-window').catch(() => {});
  });

  document.getElementById('ctxClose')?.addEventListener('click', () => {
    menu.style.display = 'none';
    backend.invoke('close-window').catch(() => {});
  });

  // Dismiss on click outside, Escape, scroll, or blur.
  document.addEventListener('click', (e) => {
    if (!menu.contains(e.target)) menu.style.display = 'none';
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') menu.style.display = 'none';
  });
  document.addEventListener('scroll', () => { menu.style.display = 'none'; }, true);
  window.addEventListener('blur', () => { menu.style.display = 'none'; });
}

// --- Startup ----------------------------------------------------------------

async function start() {
  if (!IS_DESKTOP) return;

  // Panel-specific initialization.
  if (panelKey === 'cpu') initCpuPanel();
  if (panelKey === 'clock') startClock();

  // Apply settings (theme, opacity, thresholds).
  backend.invoke('get-settings').then((s) => {
    applyOpacity(s.opacity);
    applyTheme(s.theme ?? 'dark-cyan');
    applyThresholds(s);

    if (panelKey === 'header') {
      // Populate static labels in the header panel.
      const modelEl = document.getElementById('modelName');
      if (modelEl && s.modelName) modelEl.textContent = s.modelName;
    }
  }).catch(() => {
    // If settings cannot be fetched, prefer transparent fallback over opaque gray.
    applyOpacity(0);
    applyTheme('dark-cyan');
  });

  // Static labels for header panel.
  if (panelKey === 'header') {
    updateRigName();
    updateCpuModel();
    updateGpuModel();
    Promise.all([
      backend.invoke('get-system-brand').catch(() => 'other'),
      backend.invoke('get-cpu-info').catch(() => ''),
    ]).then(([brand, cpu]) => {
      updateRigLogo(brand || 'other', cpu || '');
    });
  }

  // Listen for settings change events.
  const unlisteners = await Promise.all([
    backend.on('apply-opacity', (_e, value) => applyOpacity(value)),
    backend.on('apply-theme', (_e, key) => applyTheme(key)),
    backend.on('apply-thresholds', (_e, t) => applyThresholds(t)),
  ]);
  const unlistenStats = await backend.on('stats-broadcast', (_e, stats) => applyStats(stats));

  window.addEventListener('beforeunload', () => {
    unlisteners.forEach((fn) => fn());
    unlistenStats();
    clearTimeout(moveSaveTimer);
  });

  // Save position after each move (debounced).
  if (currentWindow?.listen) {
    currentWindow.listen('tauri://moved', scheduleSavePosition).catch(() => {});
  }
  window.addEventListener('resize', () => { syncWindowScaling(); });

  await syncWindowScaling();

  initDrag();
  initContextMenu();
}

start();
