// Disk panel renderer.
// Displays throughput plus compact utilization bars for up to three drives.

function updateDiskBars(drives) {
  const wrap = document.getElementById('diskBars');
  if (!drives || !drives.length) return;

  wrap.innerHTML = '';
  drives.slice(0, 3).forEach((d) => {
    const label = d.fs.replace(/\/dev\//, '').substring(0, 4);
    wrap.innerHTML += `<div class="bar-row">
      <div class="bar-lbl" style="width:28px;font-size:8px">${label}</div>
      <div class="bar-track"><div class="bar-fill" style="width:${d.pct}%;--c:var(--pur)"></div></div>
      <div class="bar-pct">${d.pct}%</div>
    </div>`;
  });
}

function updateDiskPanel(disk, history, pushHistory) {
  const readMBs = disk.read;
  const writeMBs = disk.write;
  const fmt = (v) => (v >= 1000 ? (v / 1000).toFixed(2) : v.toFixed(0));
  const unit = (v) => (v >= 1000 ? 'GB/s' : 'MB/s');

  pushHistory(history.disk, readMBs);

  document.getElementById('diskRead').textContent = fmt(readMBs);
  document.getElementById('diskReadU').textContent = unit(readMBs);
  document.getElementById('diskWrite').textContent = fmt(writeMBs);
  document.getElementById('diskWriteU').textContent = unit(writeMBs);

  updateDiskBars(disk.drives);
}

export { updateDiskPanel };
