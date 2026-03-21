import { backend, IS_DESKTOP } from './environment.js';

const slider = document.getElementById('slider');
const valueLabel = document.getElementById('val');
const modelNameInput = document.getElementById('modelNameInput');
const profileSelect = document.getElementById('profileSelect');
const alwaysOnTopInput = document.getElementById('alwaysOnTopInput');
const autostartInput = document.getElementById('autostartInput');
const panelToggles = document.getElementById('panelToggles');
const statusEl = document.getElementById('status');

const PANEL_KEYS = ['header', 'clock', 'cpu', 'gpu', 'ram', 'net', 'disk'];
const PANEL_LABELS = {
  header: 'Header',
  clock: 'Clock',
  cpu: 'CPU',
  gpu: 'GPU',
  ram: 'RAM',
  net: 'Network',
  disk: 'Storage',
};

let original = {
  opacity: 0.55,
  modelName: '',
  dashboardProfile: 'portrait-xl',
  alwaysOnTop: false,
  autostartEnabled: false,
  visiblePanels: [...PANEL_KEYS],
};
let isSaving = false;

// Panel ordering state — tracks all panels (visible + hidden) in user-defined order.
let panelOrder = [...PANEL_KEYS];
let hiddenPanels = new Set();
let draggingKey = null;
let dragGhost = null;
let dragOffsetX = 0;
let dragOffsetY = 0;

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
        try {
          await previewVisiblePanels(getSelectedPanels());
        } catch (error) {
          logError('preview-visible-panels', error);
        }
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
    try {
      await previewVisiblePanels(getSelectedPanels());
      setStatus('Previewing panel visibility...');
    } catch (error) {
      logError('preview-visible-panels', error);
      setStatus('Could not preview panel visibility.', 'status-err');
    }
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
  original = {
    opacity: settings.opacity ?? 0.55,
    modelName: settings.modelName ?? '',
    dashboardProfile: settings.dashboardProfile ?? 'portrait-xl',
    alwaysOnTop: settings.alwaysOnTop ?? false,
    autostartEnabled: settings.autostartEnabled ?? false,
    visiblePanels: normalizeVisiblePanels(settings.visiblePanels),
  };

  const percentage = Math.round(original.opacity * 100);
  slider.value = percentage;
  valueLabel.textContent = `${percentage}%`;
  modelNameInput.value = original.modelName;
  profileSelect.value = original.dashboardProfile;
  alwaysOnTopInput.checked = original.alwaysOnTop;
  autostartInput.checked = original.autostartEnabled;
  applyVisiblePanelsToForm(original.visiblePanels);
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
  await backend.invoke('preview-opacity', { value: original.opacity });
  await previewProfile(original.dashboardProfile);
  await previewVisiblePanels(original.visiblePanels);
  await backend.invoke('close-window');
}

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

document.getElementById('btnSave').addEventListener('click', async () => {
  if (!IS_DESKTOP || isSaving) return;

  isSaving = true;
  setStatus('Saving...');

  const opacity = parseInt(slider.value, 10) / 100;
  const modelName = modelNameInput.value.trim();
  const dashboardProfile = profileSelect.value;
  const alwaysOnTop = alwaysOnTopInput.checked;
  const autostartEnabled = autostartInput.checked;
  const selectedPanels = getSelectedPanels();

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
      visiblePanels,
    });

    original = { opacity, modelName, dashboardProfile, alwaysOnTop, autostartEnabled, visiblePanels };
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

loadSettings();
