// CPU panel renderer.
// Handles headline metrics and per-core progress bars.

import { resolveTempColor } from '../tempColors.js';

let numCores = 8;

function buildCoreBars(count) {
  // Rebuild bars only when core count changes.
  numCores = count;
  const wrap = document.getElementById('cpuCores');
  wrap.innerHTML = '';

  for (let i = 0; i < Math.min(count, 16); i++) {
    wrap.innerHTML += `<div class="bar-row">
      <div class="bar-lbl">C${i}</div>
      <div class="bar-track"><div class="bar-fill" id="c${i}" style="width:0%"></div></div>
      <div class="bar-pct" id="cp${i}">0%</div>
    </div>`;
  }
}

function initCpuPanel() {
  buildCoreBars(8);
}

function updateCpuPanel(cpu, history, pushHistory) {
  const load = cpu.load;
  pushHistory(history.cpu, load);
  const cpuTempEl = document.getElementById('cpuTemp');

  document.getElementById('cpuLoad').textContent = load;
  cpuTempEl.textContent = cpu.temp > 0 ? `${cpu.temp.toFixed(0)}°C` : '--°C';
  cpuTempEl.style.color = resolveTempColor(cpu.temp, 70, 85);
  document.getElementById('cpuFreq').textContent = cpu.freq ? `${cpu.freq.toFixed(2)} GHz` : '-- GHz';
  document.getElementById('cpuPower').textContent = cpu.power ? `${cpu.power.toFixed(0)} W` : '-- W';

  if (cpu.cores) {
    if (cpu.cores.length !== numCores) buildCoreBars(cpu.cores.length);
    cpu.cores.slice(0, 16).forEach((v, i) => {
      const fill = document.getElementById(`c${i}`);
      const pct = document.getElementById(`cp${i}`);
      if (fill) {
        fill.style.width = `${v}%`;
        pct.textContent = `${v}%`;
      }
    });
  }
}

export { initCpuPanel, updateCpuPanel };
