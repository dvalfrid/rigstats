// One-time system identity fetchers.
// These functions populate static header labels (rig name, CPU model, GPU model).

import { IS_DESKTOP, backend } from './environment.js';

function getForcedModel(kind) {
  const key = kind === 'cpu' ? 'rigstats.testCpuModel' : 'rigstats.testGpuModel';
  try {
    const value = (localStorage.getItem(key) || '').trim();
    return value || null;
  } catch (_e) {
    return null;
  }
}

function resolveVendorBadge(model, kind) {
  const text = (model || '').toLowerCase();
  if (text.includes('nvidia') || text.includes('geforce') || text.includes('rtx') || text.includes('gtx')) {
    return { src: './assets/nvidia.png', alt: `${kind} NVIDIA` };
  }
  if (text.includes('intel') || text.includes('core i') || text.includes('arc')) {
    return { src: './assets/intel.png', alt: `${kind} Intel` };
  }
  if (text.includes('amd') || text.includes('ryzen') || text.includes('radeon')) {
    return { src: './assets/AMD-Radeon-Ryzen-Symbol.png', alt: `${kind} AMD` };
  }
  return null;
}

function applyVendorBadge(badgeEl, model, kind) {
  if (!badgeEl) return;
  const badge = resolveVendorBadge(model, kind);
  if (!badge) {
    badgeEl.style.display = 'none';
    return;
  }
  badgeEl.src = badge.src;
  badgeEl.alt = badge.alt;
  badgeEl.style.display = '';
}

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
  const badge = document.getElementById('cpuVendorBadge');
  if (!el) return;

  const forced = getForcedModel('cpu');
  if (forced) {
    el.textContent = forced;
    applyVendorBadge(badge, forced, 'CPU');
    return;
  }

  if (IS_DESKTOP) {
    try {
      const model = await backend.invoke('get-cpu-info');
      if (model) {
        el.textContent = model;
        applyVendorBadge(badge, model, 'CPU');
        return;
      }
    } catch (e) {}
  }

  el.textContent = '--';
  if (badge) badge.style.display = 'none';
}

async function updateGpuModel() {
  const el = document.getElementById('gpuModel');
  const badge = document.getElementById('gpuVendorBadge');
  if (!el) return;

  const forced = getForcedModel('gpu');
  if (forced) {
    el.textContent = forced;
    applyVendorBadge(badge, forced, 'GPU');
    return;
  }

  if (IS_DESKTOP) {
    try {
      const model = await backend.invoke('get-gpu-info');
      if (model) {
        el.textContent = model;
        applyVendorBadge(badge, model, 'GPU');
        return;
      }
    } catch (e) {}
  }

  el.textContent = 'UNKNOWN GPU';
  if (badge) badge.style.display = 'none';
}

// Show or hide the header logo based on the detected system board brand.
// Logo is visible by default in HTML; this remaps known board brands.
function updateRigLogo(brand) {
  const logo = document.getElementById('rigLogo');
  if (!logo) return;

  const key = String(brand || '').toLowerCase();
  if (key === 'rog') {
    logo.src = './assets/ROG_logo_red.png';
    logo.alt = 'ROG';
    logo.style.display = '';
    return;
  }

  if (key === 'msi') {
    logo.src = './assets/msi.png';
    logo.alt = 'MSI';
    logo.style.display = '';
    return;
  }

  if (key === 'gigabyte') {
    logo.src = './assets/gigabyte.png';
    logo.alt = 'Gigabyte';
    logo.style.display = '';
    return;
  }

  if (key !== 'rog' && key !== 'msi' && key !== 'gigabyte') {
    logo.style.display = 'none';
  }
}

export { updateRigName, updateCpuModel, updateGpuModel, updateRigLogo };
