// Motherboard panel renderer.
// Fans, temperatures, and voltages displayed side-by-side in three columns.
// The block scrolls vertically if content overflows the panel height.

import { resolveTempColor } from '../tempColors.js';

// Maximum label characters kept per column type.
// Fans: "Fan #7" = 6 chars. Temps/voltages: "CPU Core" = 8 chars.
// The CSS .mb-row grid uses 8ch, which covers both cases.
const FAN_LABEL_LEN = 6;
const SENSOR_LABEL_LEN = 8;

// Board/VRM temp colour thresholds (°C). Cooler than CPU/GPU — VRMs run warm.
const TEMP_WARN_C = 60;
const TEMP_CRIT_C = 90;

// "Temperature #1" → "T1", everything else truncated to maxLen.
// Exported for unit tests.
function shortLabel(name, maxLen) {
  const trimmed = name.trim();
  const m = trimmed.match(/^Temperature #(\d+)$/i);
  if (m) return `T${m[1]}`;
  return trimmed.length <= maxLen ? trimmed : trimmed.substring(0, maxLen);
}

const EMPTY = '<div class="mb-row-lbl">--</div>';

function updateMotherboardPanel(mb) {
  const boardEl = document.getElementById('mbBoard');
  if (boardEl) boardEl.textContent = mb.board ?? '';

  const chipEl = document.getElementById('mbChip');
  if (chipEl) chipEl.textContent = mb.chip ?? '';

  renderFans(mb.fans ?? []);
  renderTemps(mb.temps ?? []);
  renderVoltages(mb.voltages ?? []);
}

function renderFans(fans) {
  const wrap = document.getElementById('mbFans');
  if (!wrap) return;

  // Backend already filters 0-RPM channels; guard here for simulator parity.
  const active = fans.filter(([, rpm]) => rpm > 0);
  if (!active.length) { wrap.innerHTML = EMPTY; return; }

  wrap.innerHTML = active.map(([label, rpm]) => `
    <div class="mb-row">
      <span class="mb-row-lbl">${shortLabel(label, FAN_LABEL_LEN)}</span>
      <span class="mb-row-val">${Math.round(rpm)}</span>
    </div>`).join('');
}

function renderTemps(temps) {
  const wrap = document.getElementById('mbTemps');
  if (!wrap) return;

  // Backend filters < 5 °C sentinels; guard here for simulator parity.
  const valid = temps.filter(([, t]) => t >= 5);
  if (!valid.length) { wrap.innerHTML = EMPTY; return; }

  // Rows use mb-row-c (compact flex) so the centering wrapper can balance them.
  wrap.innerHTML = valid.map(([label, t]) => `
    <div class="mb-row-c">
      <span class="mb-row-lbl">${shortLabel(label, SENSOR_LABEL_LEN)}</span>
      <span style="font-family:var(--mono);font-size:var(--font-ui);color:${resolveTempColor(t, TEMP_WARN_C, TEMP_CRIT_C)}">${t.toFixed(0)}°C</span>
    </div>`).join('');
}

function renderVoltages(voltages) {
  const wrap = document.getElementById('mbVoltages');
  if (!wrap) return;

  if (!voltages.length) { wrap.innerHTML = EMPTY; return; }

  wrap.innerHTML = voltages.map(([label, v]) => `
    <div class="mb-row">
      <span class="mb-row-lbl">${shortLabel(label, SENSOR_LABEL_LEN)}</span>
      <span class="mb-row-val">${v.toFixed(2)}V</span>
    </div>`).join('');
}

export { updateMotherboardPanel, shortLabel };
