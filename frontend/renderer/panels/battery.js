// Battery panel renderer.
// Gracefully handles desktops with no battery (present === false).

function batColor(pct, charging) {
  if (charging) return 'var(--accent)';
  if (pct > 50) return 'var(--grn)';
  if (pct > 20) return '#ffb300';
  return 'var(--amd)';
}

function formatTimeRemaining(mins) {
  if (mins == null || mins <= 0) return '--';
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m`;
}

function formatWatts(w) {
  if (w == null) return '--';
  return `${w.toFixed(1)} W`;
}

function updateBatteryPanel(battery) {
  const pctEl = document.getElementById('batPct');
  const barEl = document.getElementById('batBar');
  const barPctEl = document.getElementById('batBarPct');
  const statusEl = document.getElementById('batStatus');
  const timeEl = document.getElementById('batTime');
  const powerEl = document.getElementById('batPower');

  if (!battery?.present) {
    if (pctEl) { pctEl.textContent = '--'; pctEl.style.color = ''; }
    if (barEl) { barEl.style.width = '0%'; barEl.style.background = ''; }
    if (barPctEl) barPctEl.textContent = '--%';
    if (statusEl) statusEl.textContent = 'NO BATTERY';
    if (timeEl) timeEl.textContent = '--';
    if (powerEl) powerEl.textContent = '--';
    return;
  }

  const pct = battery.chargePct ?? 0;
  const charging = battery.charging ?? false;
  const color = batColor(pct, charging);

  if (pctEl) { pctEl.textContent = pct; pctEl.style.color = color; }
  if (barEl) { barEl.style.width = `${pct}%`; barEl.style.background = color; }
  if (barPctEl) barPctEl.textContent = `${pct}%`;
  if (statusEl) statusEl.textContent = charging ? 'CHARGING' : 'DISCHARGING';
  if (timeEl) timeEl.textContent = charging ? '--' : formatTimeRemaining(battery.timeRemainingMins);
  if (powerEl) powerEl.textContent = formatWatts(battery.powerW);
}

export { updateBatteryPanel };
