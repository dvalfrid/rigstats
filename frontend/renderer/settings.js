import { backend, IS_DESKTOP } from './environment.js';

const slider = document.getElementById('slider');
const valueLabel = document.getElementById('val');
const modelNameInput = document.getElementById('modelNameInput');
const profileSelect = document.getElementById('profileSelect');
const alwaysOnTopInput = document.getElementById('alwaysOnTopInput');
const statusEl = document.getElementById('status');

let original = {
  opacity: 0.55,
  modelName: '',
  dashboardProfile: 'portrait-xl',
  alwaysOnTop: false,
};
let isSaving = false;

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
  };

  const percentage = Math.round(original.opacity * 100);
  slider.value = percentage;
  valueLabel.textContent = `${percentage}%`;
  modelNameInput.value = original.modelName;
  profileSelect.value = original.dashboardProfile;
  alwaysOnTopInput.checked = original.alwaysOnTop;
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

document.getElementById('btnSave').addEventListener('click', async () => {
  if (!IS_DESKTOP || isSaving) return;

  isSaving = true;
  setStatus('Saving...');

  const opacity = parseInt(slider.value, 10) / 100;
  const modelName = modelNameInput.value.trim();
  const dashboardProfile = profileSelect.value;
  const alwaysOnTop = alwaysOnTopInput.checked;

  try {
    await backend.invoke('save-settings', {
      opacity,
      modelName,
      dashboardProfile,
      alwaysOnTop,
    });

    original = { opacity, modelName, dashboardProfile, alwaysOnTop };
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

loadSettings();