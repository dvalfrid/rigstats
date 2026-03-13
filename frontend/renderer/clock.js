// Local time and session uptime rendering.
// Runs independently from backend polling so clock remains smooth.

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

function startUptime() {
  let upSec = 0;
  setInterval(() => {
    upSec++;
    document.getElementById('uptime').textContent =
      `UP ${pad(Math.floor(upSec / 3600))}:${pad(Math.floor((upSec % 3600) / 60))}:${pad(upSec % 60)}`;
  }, 1000);
}

export { startClock, startUptime };
