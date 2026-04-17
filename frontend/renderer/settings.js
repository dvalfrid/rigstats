import { backend, IS_DESKTOP } from './environment.js';

const slider = document.getElementById('slider');
const valueLabel = document.getElementById('val');
const modelNameInput = document.getElementById('modelNameInput');
const profileSelect = document.getElementById('profileSelect');
const alwaysOnTopInput = document.getElementById('alwaysOnTopInput');
const autostartInput = document.getElementById('autostartInput');
const floatingModeInput = document.getElementById('floatingModeInput');
const floatingScaleSlider = document.getElementById('floatingScaleSlider');
const floatingScaleVal = document.getElementById('floatingScaleVal');
const floatingScaleRow = document.getElementById('floatingScaleRow');
const panelToggles = document.getElementById('panelToggles');
const statusEl = document.getElementById('status');
const btnTestAlert = document.getElementById('btnTestAlert');
const alertCooldownInput = document.getElementById('alertCooldownInput');
const warnCpuTempInput = document.getElementById('warnCpuTempInput');
const critCpuTempInput = document.getElementById('critCpuTempInput');
const warnGpuTempInput = document.getElementById('warnGpuTempInput');
const critGpuTempInput = document.getElementById('critGpuTempInput');
const warnRamTempInput = document.getElementById('warnRamTempInput');
const critRamTempInput = document.getElementById('critRamTempInput');
const warnDiskTempInput = document.getElementById('warnDiskTempInput');
const critDiskTempInput = document.getElementById('critDiskTempInput');
const notifyOnWarnInput = document.getElementById('notifyOnWarnInput');
const notifyOnCritInput = document.getElementById('notifyOnCritInput');
const themeSelect = document.getElementById('themeSelect');
const modelNameCard = document.getElementById('modelNameCard');
const opacityCard = document.getElementById('opacityCard');

const PANEL_KEYS = ['header', 'clock', 'cpu', 'gpu', 'ram', 'net', 'disk', 'motherboard', 'process'];
const PANEL_LABELS = {
  header: 'Header',
  clock: 'Clock',
  cpu: 'CPU',
  gpu: 'GPU',
  ram: 'RAM',
  net: 'Network',
  disk: 'Storage',
  motherboard: 'Motherboard',
  process: 'Processes',
};

let original = {
  opacity: 0.55,
  modelName: '',
  dashboardProfile: 'portrait-xl',
  alwaysOnTop: false,
  autostartEnabled: false,
  floatingMode: false,
  floatingPanelScale: 1.0,
  visiblePanels: [...PANEL_KEYS],
  thresholds: { cpu: {}, gpu: {}, ram: {}, disk: {} },
  alertCooldownSecs: 60,
  notifyOnWarn: true,
  notifyOnCrit: true,
  theme: 'dark-cyan',
};
let isSaving = false;
let isTogglingFloatingMode = false;
let queuedFloatingMode = null;
let previewFloatingMode = false;

// Panel ordering state — tracks all panels (visible + hidden) in user-defined order.
let panelOrder = [...PANEL_KEYS];
let hiddenPanels = new Set();
let draggingKey = null;
let dragGhost = null;
let dragOffsetX = 0;
let previewPanelsTimer = null;
let dragOffsetY = 0;

function updateFloatingScaleVisibility() {
  floatingScaleRow.style.display = floatingModeInput.checked ? 'block' : 'none';
  syncCardHeights();
}

function syncCardHeights() {
  const pairs = [
    [modelNameCard, opacityCard],
  ];

  for (const [left, right] of pairs) {
    if (!left || !right) continue;
    left.style.minHeight = '';
    right.style.minHeight = '';
    const target = Math.max(left.offsetHeight, right.offsetHeight);
    left.style.minHeight = `${target}px`;
    right.style.minHeight = `${target}px`;
  }
}


/** Reads a temp input; returns an integer 1–255 or null (blank = disabled). */
function readTempInput(el) {
  const v = parseInt(el.value, 10);
  return (!Number.isNaN(v) && v >= 1 && v <= 255) ? v : null;
}

function requestPreviewVisiblePanels(visiblePanels) {
  if (!IS_DESKTOP) return;
  const normalized = normalizeVisiblePanels(visiblePanels);
  if (previewPanelsTimer) clearTimeout(previewPanelsTimer);
  previewPanelsTimer = setTimeout(() => {
    previewVisiblePanels(normalized).catch((error) => {
      logError('preview-visible-panels', error);
      setStatus('Could not preview panel visibility.', 'status-err');
    });
  }, 120);
}

/** Writes a saved threshold value back into a number input. */
function setTempInput(el, value) {
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
      position: fixed;
      pointer-events: none;
      z-index: 9999;
      width: ${rect.width}px;
      left: ${rect.left}px;
      top: ${rect.top}px;
      opacity: 0.9;
      box-shadow: 0 6px 20px rgba(0,0,0,0.5);
      border-color: rgba(0,200,255,0.6);
      background: rgba(20,24,32,0.98);
      border-radius: 7px;
      transform: rotate(1deg) scale(1.03);
      transition: none;
    `;
    document.body.appendChild(dragGhost);
  });

  handle.addEventListener('pointermove', (e) => {
    if (draggingKey !== key) return;
    if (dragGhost) {
      dragGhost.style.left = `${e.clientX - dragOffsetX}px`;
      dragGhost.style.top = `${e.clientY - dragOffsetY}px`;
    }
    const el = document.elementFromPoint(e.clientX, e.clientY);
    const targetItem = el?.closest?.('.panel-item');
    panelToggles.querySelectorAll('.panel-item').forEach((i) => i.classList.remove('drag-over'));
    if (targetItem && targetItem !== item) {
      targetItem.classList.add('drag-over');
    }
  });

  const finishDrag = async () => {
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
        setStatus('Previewing panel visibility...');
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
  checkbox.addEventListener('change', async () => {
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
    setStatus('Previewing panel visibility...');
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
  // Visible panels appear first in their saved order; hidden panels follow.
  panelOrder = [...visible, ...hidden];
  hiddenPanels = new Set(hidden);
  renderPanelToggles();
}

async function previewVisiblePanels(visiblePanels) {
  if (!IS_DESKTOP) return;
  await backend.invoke('preview-visible-panels', { panels: normalizeVisiblePanels(visiblePanels) });
}

async function previewProfile(profile) {
  if (!IS_DESKTOP) return;
  await backend.invoke('preview-profile', { profile });
}

function setStatus(message, type = '') {
  statusEl.textContent = message;
  statusEl.className = `status ${type}`.trim();
}

function logError(context, error) {
  const message = `[settings] ${context}: ${error}`;
  console.error(message);
  if (IS_DESKTOP) {
    backend.invoke('log-frontend-error', { message }).catch(() => {});
  }
}

function applySettings(settings) {
  const t = settings.thresholds ?? {};
  original = {
    opacity: settings.opacity ?? 0.55,
    modelName: settings.modelName ?? '',
    dashboardProfile: settings.dashboardProfile ?? 'portrait-xl',
    alwaysOnTop: settings.alwaysOnTop ?? false,
    autostartEnabled: settings.autostartEnabled ?? false,
    floatingMode: settings.floatingMode ?? false,
    floatingPanelScale: settings.floatingPanelScale ?? 1.0,
    visiblePanels: normalizeVisiblePanels(settings.visiblePanels),
    thresholds: {
      cpu:  { warn: t.cpu?.warn  ?? null, crit: t.cpu?.crit  ?? null },
      gpu:  { warn: t.gpu?.warn  ?? null, crit: t.gpu?.crit  ?? null },
      ram:  { warn: t.ram?.warn  ?? null, crit: t.ram?.crit  ?? null },
      disk: { warn: t.disk?.warn ?? null, crit: t.disk?.crit ?? null },
    },
    alertCooldownSecs: settings.alertCooldownSecs ?? 60,
    notifyOnWarn: settings.notifyOnWarn ?? true,
    notifyOnCrit: settings.notifyOnCrit ?? true,
    theme: settings.theme ?? 'dark-cyan',
  };
  previewFloatingMode = original.floatingMode;

  const percentage = Math.round(original.opacity * 100);
  slider.value = percentage;
  valueLabel.textContent = `${percentage}%`;
  modelNameInput.value = original.modelName;
  profileSelect.value = original.dashboardProfile;
  alwaysOnTopInput.checked = original.alwaysOnTop;
  autostartInput.checked = original.autostartEnabled;
  floatingModeInput.checked = original.floatingMode;
  const scalePct = Math.round(original.floatingPanelScale * 100);
  floatingScaleSlider.value = scalePct;
  floatingScaleVal.textContent = `${scalePct}%`;
  updateFloatingScaleVisibility();
  applyVisiblePanelsToForm(original.visiblePanels);

  setTempInput(warnCpuTempInput,  original.thresholds.cpu.warn);
  setTempInput(critCpuTempInput,  original.thresholds.cpu.crit);
  setTempInput(warnGpuTempInput,  original.thresholds.gpu.warn);
  setTempInput(critGpuTempInput,  original.thresholds.gpu.crit);
  setTempInput(warnRamTempInput,  original.thresholds.ram.warn);
  setTempInput(critRamTempInput,  original.thresholds.ram.crit);
  setTempInput(warnDiskTempInput, original.thresholds.disk.warn);
  setTempInput(critDiskTempInput, original.thresholds.disk.crit);
  alertCooldownInput.value = original.alertCooldownSecs;
  notifyOnWarnInput.checked = original.notifyOnWarn;
  notifyOnCritInput.checked = original.notifyOnCrit;
  themeSelect.value = original.theme;
  syncCardHeights();
}

async function loadSettings() {
  if (!IS_DESKTOP) {
    setStatus('Tauri backend unavailable.', 'status-err');
    return;
  }

  let settings;
  try {
    settings = await backend.invoke('get-settings');
  } catch (error) {
    logError('get-settings', error);
    setStatus('Could not load settings.', 'status-err');
    return;
  }

  applySettings(settings);
}

async function closeWithRestore() {
  if (dragGhost) { dragGhost.remove(); dragGhost = null; }
  if (floatingModeInput.checked !== original.floatingMode) {
    await backend.invoke('toggle-floating-mode', { enabled: original.floatingMode });
  }
  await backend.invoke('preview-opacity', { value: original.opacity });
  await previewProfile(original.dashboardProfile);
  await previewVisiblePanels(original.visiblePanels);
  await backend.invoke('preview-theme', { theme: original.theme });
  if (parseFloat(floatingScaleSlider.value) / 100 !== original.floatingPanelScale) {
    await backend.invoke('preview-floating-scale', { scale: original.floatingPanelScale });
  }
  await backend.invoke('close-window');
}

themeSelect.addEventListener('change', async () => {
  if (!IS_DESKTOP) return;
  try {
    await backend.invoke('preview-theme', { theme: themeSelect.value });
  } catch (error) {
    logError('preview-theme', error);
  }
});

slider.addEventListener('input', () => {
  const percentage = parseInt(slider.value, 10);
  valueLabel.textContent = `${percentage}%`;

  if (IS_DESKTOP) {
    backend.invoke('preview-opacity', { value: percentage / 100 }).catch((error) => {
      logError('preview-opacity', error);
    });
  }
});

profileSelect.addEventListener('change', async () => {
  if (!IS_DESKTOP || isSaving) return;
  try {
    await previewProfile(profileSelect.value);
    setStatus('Previewing display profile...');
  } catch (error) {
    logError('preview-profile', error);
    setStatus('Could not preview display profile.', 'status-err');
  }
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
  } catch (error) {
    logError('toggle-floating-mode', error);
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

document.getElementById('btnSave').addEventListener('click', async () => {
  if (!IS_DESKTOP || isSaving) return;

  isSaving = true;
  setStatus('Saving...');

  const opacity = parseInt(slider.value, 10) / 100;
  const modelName = modelNameInput.value.trim();
  const dashboardProfile = profileSelect.value;
  const alwaysOnTop = alwaysOnTopInput.checked;
  const autostartEnabled = autostartInput.checked;
  const floatingMode = floatingModeInput.checked;
  const floatingPanelScale = parseInt(floatingScaleSlider.value, 10) / 100;
  const selectedPanels = getSelectedPanels();

  const thresholds = {
    cpu:  { warn: readTempInput(warnCpuTempInput),  crit: readTempInput(critCpuTempInput) },
    gpu:  { warn: readTempInput(warnGpuTempInput),  crit: readTempInput(critGpuTempInput) },
    ram:  { warn: readTempInput(warnRamTempInput),  crit: readTempInput(critRamTempInput) },
    disk: { warn: readTempInput(warnDiskTempInput), crit: readTempInput(critDiskTempInput) },
  };
  const alertCooldownSecs = Math.max(60, parseInt(alertCooldownInput.value, 10) || 60);
  const notifyOnWarn = notifyOnWarnInput.checked;
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
      opacity,
      modelName,
      dashboardProfile,
      alwaysOnTop,
      autostartEnabled,
      floatingMode,
      floatingPanelScale,
      visiblePanels,
      thresholds,
      alertCooldownSecs,
      notifyOnWarn,
      notifyOnCrit,
      theme,
    });

    original = {
      opacity, modelName, dashboardProfile, alwaysOnTop, autostartEnabled, floatingMode,
      floatingPanelScale, visiblePanels, thresholds, alertCooldownSecs, notifyOnWarn, notifyOnCrit, theme,
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

document.getElementById('btnCancel').addEventListener('click', async () => {
  if (!IS_DESKTOP || isSaving) return;

  try {
    await closeWithRestore();
  } catch (error) {
    logError('close-window', error);
    setStatus('Could not close settings.', 'status-err');
  }
});

document.addEventListener('keydown', async (event) => {
  if (event.key !== 'Escape' || !IS_DESKTOP || isSaving) return;

  try {
    await closeWithRestore();
  } catch (error) {
    logError('escape close', error);
    setStatus('Could not close settings.', 'status-err');
  }
});

btnTestAlert.addEventListener('click', async () => {
  if (!IS_DESKTOP) return;
  try {
    await backend.invoke('test-temp-alert');
    setStatus('Test notification sent.', 'status-ok');
  } catch (error) {
    logError('test-temp-alert', error);
    setStatus('Notification failed — check OS settings.', 'status-err');
  }
});

loadSettings();

window.addEventListener('resize', () => {
  syncCardHeights();
});
