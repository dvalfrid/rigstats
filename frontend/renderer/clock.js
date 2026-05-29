// Local time and backend-driven uptime rendering.
// Clock runs locally; uptime is fed from backend stats payload.

const DAYS = ['SUNDAY', 'MONDAY', 'TUESDAY', 'WEDNESDAY', 'THURSDAY', 'FRIDAY', 'SATURDAY'];

function pad(v) {
  return String(v).padStart(2, '0');
}

function updateClock() {
  const n = new Date();
  document.getElementById('clockTime').textContent = `${pad(n.getHours())}:${pad(n.getMinutes())}:${pad(n.getSeconds())}`;
  document.getElementById('clockDay').textContent = DAYS[n.getDay()];
  document.getElementById('clockDate').textContent = `${n.getFullYear()}·${pad(n.getMonth() + 1)}·${pad(n.getDate())}`;
}

function startClock() {
  updateClock();
  setInterval(updateClock, 1000);
}

// Exported for unit tests — pure, no DOM access.
function formatUptime(seconds) {
  const upSec = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  return `UP ${pad(Math.floor(upSec / 3600))}:${pad(Math.floor((upSec % 3600) / 60))}:${pad(upSec % 60)}`;
}

function setUptimeFromSeconds(seconds) {
  const el = document.getElementById('uptime');
  if (!el) return;
  el.textContent = formatUptime(seconds);
}

export { startClock, setUptimeFromSeconds, formatUptime };
