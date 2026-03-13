// GPU panel renderer.
// Updates ring gauge, bars, and thermals from normalized backend fields.

import { resolveTempColor } from '../tempColors.js';

function updateGpuPanel(gpu, history, pushHistory) {
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
  gpuTempEl.style.color = resolveTempColor(gpu.temp, 70, 82);
  gpuHotspotEl.style.color = resolveTempColor(gpu.hotspot, 90, 100);
  document.getElementById('gpuFreq').textContent = gpu.freq != null ? `${gpu.freq.toFixed(0)} MHz` : '-- MHz';

  const vramUsedGB = gpu.vramUsed != null ? (gpu.vramUsed / 1024).toFixed(1) : null;
  const vramTotalGB = (gpu.vramTotal / 1024).toFixed(0);
  document.getElementById('gpuVram').textContent = vramUsedGB
    ? `${vramUsedGB} / ${vramTotalGB} GB`
    : `-- / ${vramTotalGB} GB`;

  const vramPct = vramUsedGB ? Math.round(vramUsedGB / vramTotalGB * 100) : 0;
  document.getElementById('vramBar').style.width = `${vramPct}%`;
  document.getElementById('vramBarPct').textContent = vramPct ? `${vramPct}%` : '--%';

  document.getElementById('gpuPower').textContent = gpu.power != null ? `${gpu.power.toFixed(0)} W` : '-- W';
  document.getElementById('gpuFan').textContent = gpu.fanSpeed != null ? `${gpu.fanSpeed} RPM` : '-- RPM';
}

export { updateGpuPanel };
