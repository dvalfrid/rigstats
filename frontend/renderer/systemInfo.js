// One-time system identity fetchers.
// These functions populate static header labels (rig name, CPU model, GPU model).

import { IS_DESKTOP, backend } from './environment.js';
import { resolveVendorBadge, resolveRigLogo } from './vendorBranding.js';

function getForcedModel(kind) {
  const key = kind === 'cpu' ? 'rigstats.testCpuModel' : 'rigstats.testGpuModel';
  try {
    const value = (localStorage.getItem(key) || '').trim();
    return value || null;
  } catch (_e) {
    return null;
  }
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

  const logoInfo = resolveRigLogo(brand);
  if (!logoInfo) {
    logo.style.display = 'none';
    return;
  }

  logo.src = logoInfo.src;
  logo.alt = logoInfo.alt;
  logo.style.display = '';
}

export { updateRigName, updateCpuModel, updateGpuModel, updateRigLogo };
