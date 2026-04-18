// GPU panel renderer.
// Updates ring gauge, bars, and thermals from normalized backend fields.

import { resolveTempColor } from '../tempColors.js';
import { resolveVendorBadge } from '../vendorBranding.js';
import { backend } from '../environment.js';

const SELECTOR_STATE = {
  NONE: 'none',
  SINGLE: 'single',
  MULTI: 'multi',
};

const SELECTOR_DATASET = {
  KEY: 'gpuSelectorKey',
  WIRED: 'gpuSelectorWired',
  BUSY: 'gpuSelectorBusy',
};

const SELECTOR_FALLBACK_STYLE = 'color:var(--stat-label);font-size:9px;letter-spacing:0.5px;';
const SELECTOR_BUTTON_BASE_STYLE = 'width:11px;height:11px;border-radius:50%;padding:0;cursor:pointer;';

function getSelectorState(optionCount) {
  if (optionCount > 1) return SELECTOR_STATE.MULTI;
  if (optionCount === 1) return SELECTOR_STATE.SINGLE;
  return SELECTOR_STATE.NONE;
}

function escapeHtmlAttr(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;');
}

function buildSelectorModel(gpu = {}) {
  const availableGpus = Array.isArray(gpu.availableGpus) ? gpu.availableGpus : [];
  const selectedName = gpu.name ?? null;
  const options = availableGpus
    .filter((entry) => Array.isArray(entry) && typeof entry[0] === 'string')
    .map(([name, vramRaw]) => ({
      name,
      vramMb: Number.isFinite(vramRaw) ? vramRaw : 0,
      selected: selectedName === name,
    }));

  const state = getSelectorState(options.length);
  const key = JSON.stringify({
    state,
    selectedName,
    options: options.map((o) => [o.name, o.vramMb, o.selected]),
  });

  return { state, options, selectedName, key };
}

function selectorFallbackMarkup(state) {
  if (state === SELECTOR_STATE.SINGLE) {
    return `<span style="${SELECTOR_FALLBACK_STYLE}">1 GPU</span>`;
  }
  return `<span style="${SELECTOR_FALLBACK_STYLE}">AUTO</span>`;
}

function renderSelectorButtons(options) {
  return options.map((opt) => {
    const selectedStyle = opt.selected
      ? 'border:1px solid var(--amd);background:var(--amd);box-shadow:0 0 6px rgba(255,58,31,0.65);'
      : 'border:1px solid var(--stat-label);background:transparent;box-shadow:none;';
    return `<button data-gpu-name="${escapeHtmlAttr(opt.name)}" style="${SELECTOR_BUTTON_BASE_STYLE}${selectedStyle}" title="${escapeHtmlAttr(opt.name)}" type="button" aria-label="Välj ${escapeHtmlAttr(opt.name)}"></button>`;
  }).join('');
}

function buildGpuPreferencePayload(deviceName) {
  return { gpu_name: deviceName, gpuName: deviceName };
}

function applySelectorMarkup(selectorEl, model) {
  if (model.state === 'multi') {
    selectorEl.innerHTML = renderSelectorButtons(model.options);
  } else {
    selectorEl.innerHTML = selectorFallbackMarkup(model.state);
  }
}

function wireSelectorClick(selectorEl) {
  if (selectorEl.dataset[SELECTOR_DATASET.WIRED] === '1') return;
  selectorEl.dataset[SELECTOR_DATASET.WIRED] = '1';

  selectorEl.onclick = async (e) => {
    const target = e.target;
    if (!(target instanceof HTMLElement) || target.tagName !== 'BUTTON') return;
    e.preventDefault();

    const deviceName = target.getAttribute('data-gpu-name');
    if (!deviceName || selectorEl.dataset[SELECTOR_DATASET.BUSY] === '1') return;

    selectorEl.dataset[SELECTOR_DATASET.BUSY] = '1';
    try {
      // Send both naming styles for maximum compatibility across invoke callers.
      await backend.invoke('set_gpu_preference', buildGpuPreferencePayload(deviceName));
    } catch (err) {
      console.error('Failed to set GPU preference:', err);
    } finally {
      selectorEl.dataset[SELECTOR_DATASET.BUSY] = '0';
    }
  };
}

function renderGpuSelector(gpu) {
  const selectorEl = document.getElementById('gpuSelector');
  if (!selectorEl) return;

  const model = buildSelectorModel(gpu);
  if (selectorEl.dataset[SELECTOR_DATASET.KEY] !== model.key) {
    applySelectorMarkup(selectorEl, model);
    selectorEl.dataset[SELECTOR_DATASET.KEY] = model.key;
  }

  if (model.state === SELECTOR_STATE.MULTI) {
    wireSelectorClick(selectorEl);
  } else {
    selectorEl.onclick = null;
    selectorEl.dataset[SELECTOR_DATASET.WIRED] = '0';
  }
}

function updateGpuPanel(gpu, history, pushHistory, thresholds = {}) {
  if (gpu.name) {
    const nameEl = document.getElementById('gpuModel');
    if (nameEl) nameEl.textContent = gpu.name;
    const badgeEl = document.getElementById('gpuVendorBadge');
    if (badgeEl) {
      const badge = resolveVendorBadge(gpu.name, 'GPU');
      if (badge) {
        badgeEl.src = badge.src;
        badgeEl.alt = badge.alt;
        badgeEl.style.display = '';
      } else {
        badgeEl.style.display = 'none';
      }
    }
  }

  renderGpuSelector(gpu);

  const gpuLoad = gpu.load ?? null;
  const circumference = 263.9;
  const gpuTempEl = document.getElementById('gpuTemp');
  const gpuHotspotEl = document.getElementById('gpuHotspot');

  if (gpuLoad != null) {
    pushHistory(history.gpu, gpuLoad);
    document.getElementById('gpuRingTxt').textContent = `${gpuLoad}%`;
    document.getElementById('gpuRing').style.strokeDashoffset = circumference * (1 - gpuLoad / 100);
    document.getElementById('gpuBar').style.width = `${gpuLoad}%`;
    document.getElementById('gpuBarPct').textContent = `${gpuLoad}%`;
    document.getElementById('gpuWarn').textContent = '';
  } else {
    pushHistory(history.gpu, 0);
    document.getElementById('gpuRingTxt').textContent = '--%';
    document.getElementById('gpuWarn').textContent = 'LibreHardwareMonitor not running — GPU metrics unavailable.';
  }

  gpuTempEl.textContent = gpu.temp != null ? `${gpu.temp.toFixed(0)}°C` : '--°C';
  gpuHotspotEl.textContent = gpu.hotspot != null ? `${gpu.hotspot.toFixed(0)}°C` : '--°C';
  gpuTempEl.style.color = resolveTempColor(gpu.temp, thresholds.warn ?? 70, thresholds.crit ?? 82);
  // Hotspot thresholds are hardcoded (90/100°C) — GPU hotspot is not user-configurable
  // because its safe range differs from core temp and no separate setting exists.
  gpuHotspotEl.style.color = resolveTempColor(gpu.hotspot, 90, 100);
  document.getElementById('gpuFreq').textContent = gpu.freq != null ? `${gpu.freq.toFixed(0)} MHz` : '-- MHz';
  document.getElementById('gpuMemFreq').textContent = gpu.memFreq != null ? `${gpu.memFreq.toFixed(0)} MHz` : '-- MHz';

  const d3d3d = gpu.d3d3d ?? null;
  const d3dVdec = gpu.d3dVdec ?? null;
  document.getElementById('gpuD3dRow').style.display = d3d3d != null ? '' : 'none';
  document.getElementById('gpuVdecRow').style.display = d3dVdec != null ? '' : 'none';
  if (d3d3d != null) {
    document.getElementById('d3d3dBar').style.width = `${d3d3d}%`;
    document.getElementById('d3d3dPct').textContent = `${d3d3d.toFixed(0)}%`;
  }
  if (d3dVdec != null) {
    document.getElementById('d3dVdecBar').style.width = `${d3dVdec}%`;
    document.getElementById('d3dVdecPct').textContent = `${d3dVdec.toFixed(0)}%`;
  }

  const vramUsedGB = gpu.vramUsed != null ? (gpu.vramUsed / 1024).toFixed(1) : null;
  const vramTotalGB = gpu.vramTotal != null ? (gpu.vramTotal / 1024).toFixed(0) : null;
  document.getElementById('gpuVram').textContent = vramUsedGB != null
    ? `${vramUsedGB} / ${vramTotalGB ?? '--'} GB`
    : `-- / ${vramTotalGB ?? '--'} GB`;

  const vramPct = (vramUsedGB != null && vramTotalGB != null) ? Math.round(vramUsedGB / vramTotalGB * 100) : 0;
  document.getElementById('vramBar').style.width = `${vramPct}%`;
  document.getElementById('vramBarPct').textContent = vramPct ? `${vramPct}%` : '--%';

  document.getElementById('gpuPower').textContent = gpu.power != null ? `${gpu.power.toFixed(0)} W` : '-- W';
  document.getElementById('gpuFan').textContent = gpu.fanSpeed != null ? `${gpu.fanSpeed} RPM` : '-- RPM';
}

export {
  updateGpuPanel,
  buildSelectorModel,
  buildGpuPreferencePayload,
  selectorFallbackMarkup,
  renderSelectorButtons,
};
