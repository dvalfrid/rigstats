function updateRamPanel(ram, history, pushHistory) {
  const ramPct = Math.round(ram.used / ram.total * 100);
  const usedGB = (ram.used / 1073741824).toFixed(1);
  const totalGB = Math.round(ram.total / 1073741824);
  const freeGB = (ram.free / 1073741824).toFixed(1);

  pushHistory(history.ram, ramPct);

  document.getElementById('ramUsed').textContent = usedGB;
  document.getElementById('ramTotal').textContent = ` / ${totalGB} GB`;
  document.getElementById('ramFree').textContent = `${freeGB} GB`;
  document.getElementById('ramPct').textContent = `${ramPct}%`;
  document.getElementById('ramSpeed').textContent = ram.spec || 'RAM';
  document.getElementById('ramBar').style.width = `${ramPct}%`;
  document.getElementById('ramBarPct').textContent = `${ramPct}%`;
}

export { updateRamPanel };
