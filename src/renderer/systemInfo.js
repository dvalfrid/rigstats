import { IS_ELECTRON, ipcRenderer } from './environment.js';

async function updateRigName() {
  const rigNameEl = document.getElementById('rigName');
  if (!rigNameEl) return;

  if (IS_ELECTRON) {
    try {
      const systemName = await ipcRenderer.invoke('get-system-name');
      rigNameEl.textContent = systemName || 'UNKNOWN RIG';
      return;
    } catch (e) {
      console.error('Could not fetch computer name:', e);
    }
  }

  rigNameEl.textContent = 'RIG DASHBOARD';
}

async function updateCpuModel() {
  const el = document.getElementById('cpuModel');
  if (!el) return;

  if (IS_ELECTRON) {
    try {
      const model = await ipcRenderer.invoke('get-cpu-info');
      el.textContent = model || '--';
      return;
    } catch (e) {}
  }

  el.textContent = '--';
}

async function updateGpuModel() {
  const el = document.getElementById('gpuModel');
  if (!el) return;

  if (IS_ELECTRON) {
    try {
      const model = await ipcRenderer.invoke('get-gpu-info');
      if (model) el.textContent = model;
    } catch (e) {}
  }
}

export { updateRigName, updateCpuModel, updateGpuModel };
