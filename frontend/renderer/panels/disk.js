// Disk panel renderer.
// Displays throughput plus compact utilization bars (with temp) for up to three drives.
// When more than three drives are present the bars cycle through pages automatically,
// advancing one page every PAGE_TICKS seconds.

import { resolveTempColor } from '../tempColors.js';

const PAGE_SIZE = 3;
const PAGE_TICKS = 5;

let diskPage = 0;
let diskTicksSincePage = 0;
let lastDriveCount = 0;

function updateDiskBars(drives, thresholds = {}) {
  const wrap = document.getElementById('diskBars');
  if (!drives || !drives.length) return;

  const totalPages = Math.ceil(drives.length / PAGE_SIZE);

  // Reset to first page when the drive list changes length.
  if (drives.length !== lastDriveCount) {
    lastDriveCount = drives.length;
    diskPage = 0;
    diskTicksSincePage = 0;
  }

  // Advance page on a timer when there are multiple pages.
  if (totalPages > 1) {
    diskTicksSincePage++;
    if (diskTicksSincePage >= PAGE_TICKS) {
      diskTicksSincePage = 0;
      diskPage = (diskPage + 1) % totalPages;
    }
  }

  const pageStart = diskPage * PAGE_SIZE;
  const pageDrives = drives.slice(pageStart, pageStart + PAGE_SIZE);

  wrap.innerHTML = '';
  pageDrives.forEach((d) => {
    const label = d.fs.replace(/\/dev\//, '').substring(0, 4);
    const tempText = d.temp != null ? `${d.temp.toFixed(0)}°C` : '--°C';
    const tempColor =
      d.temp != null
        ? resolveTempColor(d.temp, thresholds.warn ?? 55, thresholds.crit ?? 70)
        : '#6f8db7';
    wrap.innerHTML += `<div class="bar-row">
      <div class="bar-lbl" style="width:4ch">${label}</div>
      <div class="bar-track"><div class="bar-fill" style="width:${d.pct}%;--c:var(--pur)"></div></div>
      <div class="bar-pct" style="width:4.5ch">${d.pct}%</div>
      <div style="font-family:var(--mono);font-size:var(--font-ui);color:${tempColor};width:5ch;text-align:right">${tempText}</div>
    </div>`;
  });

  if (totalPages > 1) {
    const dots = Array.from(
      { length: totalPages },
      (_, i) => `<span style="color:${i === diskPage ? '#fff' : '#3a4a6a'}">●</span>`,
    ).join(' ');
    wrap.innerHTML += `<div style="text-align:center;margin-top:2px;font-size:calc(var(--font-ui) * 0.5)">${dots}</div>`;
  }
}

function updateDiskPanel(disk, history, pushHistory, thresholds = {}) {
  const readMBs = disk.read;
  const writeMBs = disk.write;
  const fmt = (v) => (v >= 1000 ? (v / 1000).toFixed(2) : v.toFixed(0));
  const unit = (v) => (v >= 1000 ? 'GB/s' : 'MB/s');

  pushHistory(history.diskRead, readMBs);
  pushHistory(history.diskWrite, writeMBs);

  document.getElementById('diskRead').textContent = fmt(readMBs);
  document.getElementById('diskReadU').textContent = unit(readMBs);
  document.getElementById('diskWrite').textContent = fmt(writeMBs);
  document.getElementById('diskWriteU').textContent = unit(writeMBs);

  updateDiskBars(disk.drives, thresholds);
}

export { updateDiskPanel };
