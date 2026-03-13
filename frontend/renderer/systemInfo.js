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
  const badge = document.querySelector('.amd-badge-cpu');
  if (!el) return;

  if (IS_DESKTOP) {
    try {
      const model = await backend.invoke('get-cpu-info');
      if (model) {
        el.textContent = model;
        if (badge) badge.style.display = /amd|ryzen/i.test(model) ? '' : 'none';
        return;
      }
    } catch (e) {}
  }

  el.textContent = '--';
  if (badge) badge.style.display = 'none';
}

async function updateGpuModel() {
  const el = document.getElementById('gpuModel');
  const badge = document.querySelector('.amd-badge');
  if (!el) return;

  if (IS_DESKTOP) {
    try {
      const model = await backend.invoke('get-gpu-info');
      if (model) {
        el.textContent = model;
        if (badge) badge.style.display = /amd|radeon/i.test(model) ? '' : 'none';
        return;
      }
    } catch (e) {}
  }

  el.textContent = 'UNKNOWN GPU';
  if (badge) badge.style.display = 'none';
}

// Show or hide the header logo based on the detected system board brand.
// Logo is visible by default in HTML; this hides it for non-ROG boards.
function updateRigLogo(brand) {
  const logo = document.getElementById('rigLogo');
  if (!logo) return;

  if (brand !== 'rog') {
    logo.style.display = 'none';
  }
  // brand === 'rog': leave visible (already shown by default in HTML)
}

export { updateRigName, updateCpuModel, updateGpuModel, updateRigLogo };
