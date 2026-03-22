// One-time system identity fetchers.
// These functions populate static header labels (rig name, CPU model, GPU model).

import { IS_DESKTOP, backend } from './environment.js';
import { resolveArchLogo, resolveVendorBadge, resolveRigLogo } from './vendorBranding.js';

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
    } catch (_e) {}
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
    } catch (_e) {}
  }

  el.textContent = 'UNKNOWN GPU';
  if (badge) badge.style.display = 'none';
}

// Fallback chain:
// 1. Recognised brand with logo image.
// 2. CPU architecture (Intel / AMD) logo derived from CPU model string.
// 3. Nothing — both elements hidden.
function updateRigLogo(brand, cpuModel) {
  const logo = document.getElementById('rigLogo');
  const brandText = document.getElementById('rigBrandText');
  if (!logo || !brandText) return;

  const logoInfo = resolveRigLogo(brand);
  if (logoInfo) {
    logo.src = logoInfo.src;
    logo.alt = logoInfo.alt;
    logo.style.display = '';
    brandText.style.display = 'none';
    return;
  }

  const archLogo = resolveArchLogo(cpuModel);
  if (archLogo) {
    logo.src = archLogo.src;
    logo.alt = archLogo.alt;
    logo.style.display = '';
    brandText.style.display = 'none';
    return;
  }

  logo.style.display = 'none';
  brandText.style.display = 'none';
}

export { updateRigName, updateCpuModel, updateGpuModel, updateRigLogo };
