// GPU panel renderer.
// Updates ring gauge, bars, and thermals from normalized backend fields.

import { resolveTempColor } from '../tempColors.js';

function updateGpuPanel(gpu, history, pushHistory, thresholds = {}) {
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

export { updateGpuPanel };
