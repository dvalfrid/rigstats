import { backend, IS_DESKTOP } from './environment.js';

// --- DOM refs ----------------------------------------------------------------

const slider              = document.getElementById('slider');
const valueLabel          = document.getElementById('val');
const modelNameInput      = document.getElementById('modelNameInput');
const profileSelect       = document.getElementById('profileSelect');
const windowLayerSelect   = document.getElementById('windowLayerSelect');
const autostartInput      = document.getElementById('autostartInput');
const floatingModeInput   = document.getElementById('floatingModeInput');
const floatingScaleSlider = document.getElementById('floatingScaleSlider');
const floatingScaleVal    = document.getElementById('floatingScaleVal');
const floatingScaleRow    = document.getElementById('floatingScaleRow');
const panelToggles        = document.getElementById('panelToggles');
const statusEl            = document.getElementById('status');
const btnTestAlert        = document.getElementById('btnTestAlert');
const alertCooldownInput  = document.getElementById('alertCooldownInput');
const warnCpuTempInput    = document.getElementById('warnCpuTempInput');
const critCpuTempInput    = document.getElementById('critCpuTempInput');
const warnGpuTempInput    = document.getElementById('warnGpuTempInput');
const critGpuTempInput    = document.getElementById('critGpuTempInput');
const warnRamTempInput    = document.getElementById('warnRamTempInput');
const critRamTempInput    = document.getElementById('critRamTempInput');
const warnDiskTempInput   = document.getElementById('warnDiskTempInput');
const critDiskTempInput   = document.getElementById('critDiskTempInput');
const warnBatteryInput    = document.getElementById('warnBatteryInput');
const critBatteryInput    = document.getElementById('critBatteryInput');
const notifyOnCritInput   = document.getElementById('notifyOnCritInput');
const themeSelect         = document.getElementById('themeSelect');

// --- Panel config ------------------------------------------------------------

const PANEL_KEYS = ['header', 'clock', 'cpu', 'gpu', 'ram', 'net', 'disk', 'motherboard', 'process', 'battery'];
const PANEL_LABELS = {
  header: 'Header', clock: 'Clock', cpu: 'CPU', gpu: 'GPU', ram: 'RAM',
  net: 'Network', disk: 'Storage', motherboard: 'Motherboard', process: 'Processes', battery: 'Battery',
};

// --- Tab switching -----------------------------------------------------------

const TAB_STORAGE_KEY = 'rigstats.settingsTab';

function switchTab(name) {
  document.querySelectorAll('.tab').forEach((t) => t.classList.toggle('active', t.dataset.tab === name));
  document.querySelectorAll('.tab-panel').forEach((p) => {
    p.classList.toggle('active', p.id === `tab-${name}`);
  });
  try { localStorage.setItem(TAB_STORAGE_KEY, name); } catch (_e) {}
}

document.querySelectorAll('.tab').forEach((btn) => {
  btn.addEventListener('click', () => switchTab(btn.dataset.tab));
});

// Restore last active tab.
try {
  const saved = localStorage.getItem(TAB_STORAGE_KEY);
  if (saved && document.getElementById(`tab-${saved}`)) switchTab(saved);
} catch (_e) {}

// --- State ------------------------------------------------------------------

let original = {
  opacity: 0.55,
  modelName: '',
  dashboardProfile: 'portrait-xl',
  windowLayer: 'normal',
  autostartEnabled: false,
  floatingMode: false,
  floatingPanelScale: 1.0,
  visiblePanels: [...PANEL_KEYS],
  thresholds: { cpu: {}, gpu: {}, ram: {}, disk: {}, battery: {} },
  alertCooldownSecs: 60,
  notifyOnCrit: true,
  theme: 'dark-cyan',
};
let isSaving = false;
let isTogglingFloatingMode = false;
let queuedFloatingMode = null;
let previewFloatingMode = false;

// Panel ordering state.
let panelOrder = [...PANEL_KEYS];
let hiddenPanels = new Set();
let draggingKey = null;
let dragGhost = null;
let dragOffsetX = 0;
let previewPanelsTimer = null;
let dragOffsetY = 0;

// --- Helpers ----------------------------------------------------------------

/** Reads a threshold number input; returns integer 1–255 or null (blank = disabled). */
function readThresholdInput(el) {
  if (!el) return null;
  const v = parseInt(el.value, 10);
  return (!Number.isNaN(v) && v >= 1 && v <= 255) ? v : null;
}

function setThresholdInput(el, value) {
  if (!el) return;
  el.value = (value != null) ? String(value) : '';
}

function normalizeVisiblePanels(value) {
  const list = Array.isArray(value) ? value : [];
  const normalized = list
    .map((v) => String(v).trim().toLowerCase())
    .filter((v, idx, arr) => v && PANEL_KEYS.includes(v) && arr.indexOf(v) === idx);
  return normalized.length > 0 ? normalized : [...PANEL_KEYS];
}

function getSelectedPanels() {
  return panelOrder.filter((k) => !hiddenPanels.has(k));
}

function setStatus(message, type = '') {
  statusEl.textContent = message;
  statusEl.className = `status ${type}`.trim();
}

function logError(context, error) {
  const message = `[settings] ${context}: ${error}`;
  console.error(message);
  if (IS_DESKTOP) backend.invoke('log-frontend-error', { message }).catch(() => {});
}

function updateFloatingScaleVisibility() {
  floatingScaleRow.style.display = floatingModeInput.checked ? 'block' : 'none';
}

// --- Panel drag-and-drop ----------------------------------------------------

function attachPanelItemEvents(item) {
  const key = item.dataset.panelKey;
  const handle = item.querySelector('.panel-drag-handle');

  handle.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    draggingKey = key;
    handle.setPointerCapture(e.pointerId);
    const rect = item.getBoundingClientRect();
    dragOffsetX = e.clientX - rect.left;
    dragOffsetY = e.clientY - rect.top;
    dragGhost = item.cloneNode(true);
    item.classList.add('dragging');
    dragGhost.style.cssText = `
      position:fixed;pointer-events:none;z-index:9999;
      width:${rect.width}px;left:${rect.left}px;top:${rect.top}px;
      opacity:0.9;box-shadow:0 6px 20px rgba(0,0,0,0.5);
      border-color:rgba(96,205,255,0.6);background:rgba(20,24,32,0.98);
      border-radius:7px;transform:rotate(1deg) scale(1.03);transition:none;
    `;
    document.body.appendChild(dragGhost);
  });

  handle.addEventListener('pointermove', (e) => {
    if (draggingKey !== key) return;
    if (dragGhost) {
      dragGhost.style.left = `${e.clientX - dragOffsetX}px`;
      dragGhost.style.top  = `${e.clientY - dragOffsetY}px`;
    }
    const el = document.elementFromPoint(e.clientX, e.clientY);
    const targetItem = el?.closest?.('.panel-item');
    panelToggles.querySelectorAll('.panel-item').forEach((i) => i.classList.remove('drag-over'));
    if (targetItem && targetItem !== item) targetItem.classList.add('drag-over');
  });

  const finishDrag = () => {
    if (draggingKey !== key) return;
    draggingKey = null;
    item.classList.remove('dragging');
    if (dragGhost) { dragGhost.remove(); dragGhost = null; }
    const target = panelToggles.querySelector('.panel-item.drag-over');
    panelToggles.querySelectorAll('.panel-item').forEach((i) => i.classList.remove('drag-over'));
    if (target && target !== item) {
      const srcIdx = panelOrder.indexOf(key);
      const dstIdx = panelOrder.indexOf(target.dataset.panelKey);
      if (srcIdx !== -1 && dstIdx !== -1) {
        panelOrder.splice(srcIdx, 1);
        panelOrder.splice(dstIdx, 0, key);
        renderPanelToggles();
        requestPreviewVisiblePanels(getSelectedPanels());
        setStatus('Previewing panel order…');
      }
    }
  };

  handle.addEventListener('pointerup', finishDrag);
  handle.addEventListener('pointercancel', () => {
    if (draggingKey !== key) return;
    draggingKey = null;
    item.classList.remove('dragging');
    if (dragGhost) { dragGhost.remove(); dragGhost = null; }
    panelToggles.querySelectorAll('.panel-item').forEach((i) => i.classList.remove('drag-over'));
  });

  const checkbox = item.querySelector('input[type=checkbox]');
  checkbox.addEventListener('change', () => {
    if (!checkbox.checked) {
      if (getSelectedPanels().length <= 1) {
        checkbox.checked = true;
        setStatus('At least one panel must remain visible.', 'status-err');
        return;
      }
      hiddenPanels.add(key);
      item.classList.add('hidden-panel');
    } else {
      hiddenPanels.delete(key);
      item.classList.remove('hidden-panel');
    }
    requestPreviewVisiblePanels(getSelectedPanels());
    setStatus('Previewing panel visibility…');
  });
}

function renderPanelToggles() {
  panelToggles.innerHTML = panelOrder.map((key) => {
    const hidden = hiddenPanels.has(key);
    return `<div class="panel-item${hidden ? ' hidden-panel' : ''}" data-panel-key="${key}">
      <span class="panel-drag-handle" title="Drag to reorder">≡</span>
      <span class="panel-item-label">${PANEL_LABELS[key]}</span>
      <input type="checkbox" class="toggle-input" data-panel-key="${key}"${hidden ? '' : ' checked'}>
    </div>`;
  }).join('');
  panelToggles.querySelectorAll('.panel-item').forEach(attachPanelItemEvents);
}

function applyVisiblePanelsToForm(visiblePanels) {
  const visible = normalizeVisiblePanels(visiblePanels);
  const hidden = PANEL_KEYS.filter((k) => !visible.includes(k));
  panelOrder = [...visible, ...hidden];
  hiddenPanels = new Set(hidden);
  renderPanelToggles();
}

// --- Preview helpers --------------------------------------------------------

function requestPreviewVisiblePanels(visiblePanels) {
  if (!IS_DESKTOP) return;
  const normalized = normalizeVisiblePanels(visiblePanels);
  if (previewPanelsTimer) clearTimeout(previewPanelsTimer);
  previewPanelsTimer = setTimeout(() => {
    backend.invoke('preview-visible-panels', { panels: normalized }).catch((e) => {
      logError('preview-visible-panels', e);
      setStatus('Could not preview panel visibility.', 'status-err');
    });
  }, 120);
}

async function previewProfile(profile) {
  if (!IS_DESKTOP) return;
  await backend.invoke('preview-profile', { profile });
}

// --- Load / apply settings --------------------------------------------------

function applySettings(settings) {
  const t = settings.thresholds ?? {};
  original = {
    opacity:           settings.opacity ?? 0.55,
    modelName:         settings.modelName ?? '',
    dashboardProfile:  settings.dashboardProfile ?? 'portrait-xl',
    windowLayer:       settings.windowLayer ?? 'normal',
    autostartEnabled:  settings.autostartEnabled ?? false,
    floatingMode:      settings.floatingMode ?? false,
    floatingPanelScale: settings.floatingPanelScale ?? 1.0,
    visiblePanels:     normalizeVisiblePanels(settings.visiblePanels),
    thresholds: {
      cpu:     { warn: t.cpu?.warn     ?? null, crit: t.cpu?.crit     ?? null },
      gpu:     { warn: t.gpu?.warn     ?? null, crit: t.gpu?.crit     ?? null },
      ram:     { warn: t.ram?.warn     ?? null, crit: t.ram?.crit     ?? null },
      disk:    { warn: t.disk?.warn    ?? null, crit: t.disk?.crit    ?? null },
      battery: { warn: t.battery?.warn ?? null, crit: t.battery?.crit ?? null },
    },
    alertCooldownSecs: settings.alertCooldownSecs ?? 60,
    notifyOnCrit:      settings.notifyOnCrit ?? true,
    theme:             settings.theme ?? 'dark-cyan',
  };
  previewFloatingMode = original.floatingMode;

  const pct = Math.round(original.opacity * 100);
  slider.value = pct;
  valueLabel.textContent = `${pct}%`;
  modelNameInput.value = original.modelName;
  profileSelect.value = original.dashboardProfile;
  windowLayerSelect.value = original.windowLayer;
  autostartInput.checked = original.autostartEnabled;
  floatingModeInput.checked = original.floatingMode;
  const scalePct = Math.round(original.floatingPanelScale * 100);
  floatingScaleSlider.value = scalePct;
  floatingScaleVal.textContent = `${scalePct}%`;
  updateFloatingScaleVisibility();
  applyVisiblePanelsToForm(original.visiblePanels);

  setThresholdInput(warnCpuTempInput,  original.thresholds.cpu.warn);
  setThresholdInput(critCpuTempInput,  original.thresholds.cpu.crit);
  setThresholdInput(warnGpuTempInput,  original.thresholds.gpu.warn);
  setThresholdInput(critGpuTempInput,  original.thresholds.gpu.crit);
  setThresholdInput(warnRamTempInput,  original.thresholds.ram.warn);
  setThresholdInput(critRamTempInput,  original.thresholds.ram.crit);
  setThresholdInput(warnDiskTempInput, original.thresholds.disk.warn);
  setThresholdInput(critDiskTempInput, original.thresholds.disk.crit);
  setThresholdInput(warnBatteryInput,  original.thresholds.battery.warn);
  setThresholdInput(critBatteryInput,  original.thresholds.battery.crit);
  alertCooldownInput.value = original.alertCooldownSecs;
  notifyOnCritInput.checked = original.notifyOnCrit;
  themeSelect.value = original.theme;
}

async function loadSettings() {
  if (!IS_DESKTOP) { setStatus('Tauri backend unavailable.', 'status-err'); return; }
  try {
    applySettings(await backend.invoke('get-settings'));
  } catch (error) {
    logError('get-settings', error);
    setStatus('Could not load settings.', 'status-err');
  }
}

// --- Restore on cancel ------------------------------------------------------

async function closeWithRestore() {
  if (dragGhost) { dragGhost.remove(); dragGhost = null; }
  if (floatingModeInput.checked !== original.floatingMode) {
    await backend.invoke('toggle-floating-mode', { enabled: original.floatingMode });
  }
  await backend.invoke('preview-opacity', { value: original.opacity });
  await previewProfile(original.dashboardProfile);
  await backend.invoke('preview-visible-panels', { panels: normalizeVisiblePanels(original.visiblePanels) });
  await backend.invoke('preview-theme', { theme: original.theme });
  if (parseFloat(floatingScaleSlider.value) / 100 !== original.floatingPanelScale) {
    await backend.invoke('preview-floating-scale', { scale: original.floatingPanelScale });
  }
  await backend.invoke('close-window');
}

// --- Live preview listeners -------------------------------------------------

themeSelect.addEventListener('change', async () => {
  if (!IS_DESKTOP) return;
  try { await backend.invoke('preview-theme', { theme: themeSelect.value }); }
  catch (e) { logError('preview-theme', e); }
});

slider.addEventListener('input', () => {
  const pct = parseInt(slider.value, 10);
  valueLabel.textContent = `${pct}%`;
  if (IS_DESKTOP) backend.invoke('preview-opacity', { value: pct / 100 }).catch((e) => logError('preview-opacity', e));
});

profileSelect.addEventListener('change', async () => {
  if (!IS_DESKTOP || isSaving) return;
  try { await previewProfile(profileSelect.value); setStatus('Previewing display profile…'); }
  catch (e) { logError('preview-profile', e); setStatus('Could not preview display profile.', 'status-err'); }
});

floatingModeInput.addEventListener('change', async () => {
  updateFloatingScaleVisibility();
  if (!IS_DESKTOP || isSaving) return;
  queuedFloatingMode = floatingModeInput.checked;
  if (isTogglingFloatingMode) return;
  isTogglingFloatingMode = true;
  floatingModeInput.disabled = true;
  try {
    while (queuedFloatingMode != null) {
      const target = queuedFloatingMode;
      queuedFloatingMode = null;
      await backend.invoke('toggle-floating-mode', { enabled: target });
      previewFloatingMode = target;
      setStatus('');
    }
  } catch (e) {
    logError('toggle-floating-mode', e);
    setStatus('Could not toggle floating mode preview.', 'status-err');
    floatingModeInput.checked = previewFloatingMode;
    updateFloatingScaleVisibility();
  } finally {
    isTogglingFloatingMode = false;
    floatingModeInput.disabled = false;
  }
});

floatingScaleSlider.addEventListener('input', async () => {
  const pct = parseInt(floatingScaleSlider.value, 10);
  floatingScaleVal.textContent = `${pct}%`;
  if (IS_DESKTOP) await backend.invoke('preview-floating-scale', { scale: pct / 100 });
});

// --- Save -------------------------------------------------------------------

document.getElementById('btnSave').addEventListener('click', async () => {
  if (!IS_DESKTOP || isSaving) return;
  isSaving = true;
  setStatus('Saving…');

  const opacity           = parseInt(slider.value, 10) / 100;
  const modelName         = modelNameInput.value.trim();
  const dashboardProfile  = profileSelect.value;
  const windowLayer       = windowLayerSelect.value;
  const autostartEnabled  = autostartInput.checked;
  const floatingMode      = floatingModeInput.checked;
  const floatingPanelScale = parseInt(floatingScaleSlider.value, 10) / 100;
  const selectedPanels    = getSelectedPanels();
  const thresholds = {
    cpu:     { warn: readThresholdInput(warnCpuTempInput),  crit: readThresholdInput(critCpuTempInput) },
    gpu:     { warn: readThresholdInput(warnGpuTempInput),  crit: readThresholdInput(critGpuTempInput) },
    ram:     { warn: readThresholdInput(warnRamTempInput),  crit: readThresholdInput(critRamTempInput) },
    disk:    { warn: readThresholdInput(warnDiskTempInput), crit: readThresholdInput(critDiskTempInput) },
    battery: { warn: readThresholdInput(warnBatteryInput),  crit: readThresholdInput(critBatteryInput) },
  };
  const alertCooldownSecs = Math.max(60, parseInt(alertCooldownInput.value, 10) || 60);
  const notifyOnWarn = false; // warnings never fire; only critical alerts are sent
  const notifyOnCrit = notifyOnCritInput.checked;
  const theme = themeSelect.value;

  if (selectedPanels.length === 0) {
    setStatus('Select at least one panel.', 'status-err');
    isSaving = false;
    return;
  }

  const visiblePanels = normalizeVisiblePanels(selectedPanels);
  try {
    await backend.invoke('save-settings', {
      opacity, modelName, dashboardProfile, windowLayer, autostartEnabled,
      floatingMode, floatingPanelScale, visiblePanels, thresholds,
      alertCooldownSecs, notifyOnWarn, notifyOnCrit, theme,
    });
    original = {
      opacity, modelName, dashboardProfile, windowLayer, autostartEnabled,
      floatingMode, floatingPanelScale, visiblePanels, thresholds,
      alertCooldownSecs, notifyOnCrit, theme,
    };
    setStatus('Saved', 'status-ok');
    await backend.invoke('close-window');
  } catch (error) {
    logError('save-settings', error);
    setStatus(`Save failed: ${error}`, 'status-err');
  } finally {
    isSaving = false;
  }
});

// --- Cancel / Escape --------------------------------------------------------

document.getElementById('btnCancel').addEventListener('click', async () => {
  if (!IS_DESKTOP || isSaving) return;
  try { await closeWithRestore(); }
  catch (e) { logError('close-window', e); setStatus('Could not close settings.', 'status-err'); }
});

document.addEventListener('keydown', async (event) => {
  if (event.key !== 'Escape' || !IS_DESKTOP || isSaving) return;
  try { await closeWithRestore(); }
  catch (e) { logError('escape close', e); setStatus('Could not close settings.', 'status-err'); }
});

// --- Test notification ------------------------------------------------------

btnTestAlert.addEventListener('click', async () => {
  if (!IS_DESKTOP) return;
  try {
    await backend.invoke('test-temp-alert');
    setStatus('Test notification sent.', 'status-ok');
  } catch (e) {
    logError('test-temp-alert', e);
    setStatus('Notification failed — check OS settings.', 'status-err');
  }
});

// --- Init -------------------------------------------------------------------

loadSettings();
