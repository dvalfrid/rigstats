// One-time system identity fetchers.
// These functions populate static header labels (rig name, CPU model, GPU model).

import { IS_DESKTOP, backend } from './environment.js';

async function updateRigName() {
  const rigNameEl = document.getElementById('rigName');
  if (!rigNameEl) return;

  if (IS_DESKTOP) {
    try {
      const systemName = await backend.invoke('get-system-name');
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

  if (IS_DESKTOP) {
    try {
      const model = await backend.invoke('get-cpu-info');
      el.textContent = model || '--';
      return;
    } catch (e) {}
  }

  el.textContent = '--';
}

async function updateGpuModel() {
  const el = document.getElementById('gpuModel');
  if (!el) return;

  if (IS_DESKTOP) {
    try {
      const model = await backend.invoke('get-gpu-info');
      if (model) el.textContent = model;
    } catch (e) {}
  }
}

export { updateRigName, updateCpuModel, updateGpuModel };
