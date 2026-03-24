// Disk panel renderer.
// Displays throughput plus compact utilization bars (with temp) for up to three drives.

import { resolveTempColor } from '../tempColors.js';

function updateDiskBars(drives, thresholds = {}) {
  const wrap = document.getElementById('diskBars');
  if (!drives || !drives.length) return;

  wrap.innerHTML = '';
  drives.slice(0, 3).forEach((d) => {
    const label = d.fs.replace(/\/dev\//, '').substring(0, 4);
    const tempText = d.temp != null ? `${d.temp.toFixed(0)}°C` : '--°C';
    const tempColor = d.temp != null ? resolveTempColor(d.temp, thresholds.warn ?? 55, thresholds.crit ?? 70) : '#6f8db7';
    wrap.innerHTML += `<div class="bar-row">
      <div class="bar-lbl" style="width:36px;font-size:14px">${label}</div>
      <div class="bar-track"><div class="bar-fill" style="width:${d.pct}%;--c:var(--pur)"></div></div>
      <div class="bar-pct" style="font-size:14px;width:40px">${d.pct}%</div>
      <div style="font-family:var(--mono);font-size:14px;color:${tempColor};width:44px;text-align:right">${tempText}</div>
    </div>`;
  });
}

function updateDiskPanel(disk, history, pushHistory, thresholds = {}) {
  const readMBs = disk.read;
  const writeMBs = disk.write;
  const fmt = (v) => (v >= 1000 ? (v / 1000).toFixed(2) : v.toFixed(0));
  const unit = (v) => (v >= 1000 ? 'GB/s' : 'MB/s');

  pushHistory(history.disk, readMBs);

  document.getElementById('diskRead').textContent = fmt(readMBs);
  document.getElementById('diskReadU').textContent = unit(readMBs);
  document.getElementById('diskWrite').textContent = fmt(writeMBs);
  document.getElementById('diskWriteU').textContent = unit(writeMBs);

  updateDiskBars(disk.drives, thresholds);
}

export { updateDiskPanel };
