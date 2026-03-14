import { backend, IS_DESKTOP } from './environment.js';

const slider = document.getElementById('slider');
const valueLabel = document.getElementById('val');
const modelNameInput = document.getElementById('modelNameInput');
const profileSelect = document.getElementById('profileSelect');
const alwaysOnTopInput = document.getElementById('alwaysOnTopInput');
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
  visiblePanels: [...PANEL_KEYS],
};
let isSaving = false;

function normalizeVisiblePanels(value) {
  const list = Array.isArray(value) ? value : [];
  const normalized = list
    .map((v) => String(v).trim().toLowerCase())
    .filter((v, idx, arr) => v && PANEL_KEYS.includes(v) && arr.indexOf(v) === idx);
  return normalized.length > 0 ? normalized : [...PANEL_KEYS];
}

function renderPanelToggles() {
  panelToggles.innerHTML = PANEL_KEYS.map((key) => (
    `<label class="panel-toggle" for="panel-${key}">
      <input type="checkbox" id="panel-${key}" data-panel-key="${key}">
      <span>${PANEL_LABELS[key]}</span>
    </label>`
  )).join('');
}

function getSelectedPanels() {
  return [...panelToggles.querySelectorAll('input[data-panel-key]:checked')]
    .map((input) => input.dataset.panelKey)
    .filter(Boolean);
}

function applyVisiblePanelsToForm(visiblePanels) {
  const allowed = new Set(normalizeVisiblePanels(visiblePanels));
  panelToggles.querySelectorAll('input[data-panel-key]').forEach((input) => {
    input.checked = allowed.has(input.dataset.panelKey);
  });
}

async function previewVisiblePanels(visiblePanels) {
  if (!IS_DESKTOP) return;
  await backend.invoke('preview-visible-panels', { panels: normalizeVisiblePanels(visiblePanels) });
}

function setStatus(message, type = '') {
  statusEl.textContent = message;
  statusEl.className = `status ${type}`.trim();
}

function applySettings(settings) {
  original = {
    opacity: settings.opacity ?? 0.55,
    modelName: settings.modelName ?? '',
    dashboardProfile: settings.dashboardProfile ?? 'portrait-xl',
    alwaysOnTop: settings.alwaysOnTop ?? false,
    visiblePanels: normalizeVisiblePanels(settings.visiblePanels),
  };

  const percentage = Math.round(original.opacity * 100);
  slider.value = percentage;
  valueLabel.textContent = `${percentage}%`;
  modelNameInput.value = original.modelName;
  profileSelect.value = original.dashboardProfile;
  alwaysOnTopInput.checked = original.alwaysOnTop;
  applyVisiblePanelsToForm(original.visiblePanels);
}

async function loadSettings() {
  if (!IS_DESKTOP) {
    setStatus('Tauri backend unavailable.', 'status-err');
    return;
  }

  try {
    applySettings(await backend.invoke('get-settings'));
  } catch (error) {
    console.error('get-settings failed:', error);
    setStatus('Could not load settings.', 'status-err');
  }
}

async function closeWithRestore() {
  await backend.invoke('preview-opacity', { value: original.opacity });
  await previewVisiblePanels(original.visiblePanels);
  await backend.invoke('close-window');
}

slider.addEventListener('input', () => {
  const percentage = parseInt(slider.value, 10);
  valueLabel.textContent = `${percentage}%`;

  if (IS_DESKTOP) {
    backend.invoke('preview-opacity', { value: percentage / 100 }).catch((error) => {
      console.error('preview-opacity failed:', error);
    });
  }
});

panelToggles.addEventListener('change', async (event) => {
  const input = event.target;
  if (!input || input.tagName !== 'INPUT' || input.dataset.panelKey == null) return;

  const selectedPanels = getSelectedPanels();
  if (selectedPanels.length === 0) {
    input.checked = true;
    setStatus('At least one panel must remain visible.', 'status-err');
    return;
  }

  try {
    await previewVisiblePanels(selectedPanels);
    setStatus('Previewing panel visibility...');
  } catch (error) {
    console.error('preview-visible-panels failed:', error);
    setStatus('Could not preview panel visibility.', 'status-err');
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
      visiblePanels,
    });

    original = { opacity, modelName, dashboardProfile, alwaysOnTop, visiblePanels };
    setStatus('Saved', 'status-ok');
    await backend.invoke('close-window');
  } catch (error) {
    console.error('save-settings failed:', error);
    setStatus('Save failed. Please try again.', 'status-err');
  } finally {
    isSaving = false;
  }
});

document.getElementById('btnCancel').addEventListener('click', async () => {
  if (!IS_DESKTOP || isSaving) return;

  try {
    await closeWithRestore();
  } catch (error) {
    console.error('close-window failed:', error);
    setStatus('Could not close settings.', 'status-err');
  }
});

document.addEventListener('keydown', async (event) => {
  if (event.key !== 'Escape' || !IS_DESKTOP || isSaving) return;

  try {
    await closeWithRestore();
  } catch (error) {
    console.error('escape close failed:', error);
    setStatus('Could not close settings.', 'status-err');
  }
});

renderPanelToggles();
loadSettings();