// RAM panel renderer.
// Converts byte values from backend into human-readable GB values.

import { resolveTempColor } from '../tempColors.js';

function parseRamType(spec, details) {
  const match = (spec || '').match(/(DDR\d*)/i);
  if (match) return match[1].toUpperCase();

  const source = `${spec || ''} ${details || ''}`.toUpperCase();
  if (source.includes('DDR5') || source.includes('D5') || source.includes('PC5')) return 'DDR5';
  if (source.includes('DDR4') || source.includes('D4') || source.includes('PC4')) return 'DDR4';
  if (source.includes('DDR3') || source.includes('D3') || source.includes('PC3')) return 'DDR3';

  return '--';
}

function parseRamSpeed(spec, details) {
  const match = (spec || '').match(/(\d+\s*MT\/s)/i);
  if (match) return match[1].toUpperCase();

  // Fallback: infer from common part-number formats (e.g. UD5-6000, 5600, 6400).
  const detailsText = (details || '').toUpperCase();
  const maybeSpeed = detailsText.match(/\b(4[0-9]{3}|5[0-9]{3}|6[0-9]{3}|7[0-9]{3})\b/);
  if (maybeSpeed) return `${maybeSpeed[1]} MT/S`;

  return '--';
}

function parseRamDimms(spec, details) {
  const match = (spec || '').match(/\((\d+)\s*DIMMs\)/i);
  if (match) return `${match[1]} DIMMs`;

  const firstDetailsPart = (details || '').split('|')[0]?.trim() || '';
  const dimmsMatch = firstDetailsPart.match(/^(\d+)\s*DIMMs$/i);
  if (dimmsMatch) return `${dimmsMatch[1]} DIMMs`;

  const layoutMatch = firstDetailsPart.match(/^(\d+)x\d+/i);
  if (layoutMatch) return `${layoutMatch[1]} DIMMs`;

  return '--';
}

function parseRamDetails(details) {
  const parts = (details || '')
    .split('|')
    .map((v) => v.trim())
    .filter(Boolean);

  return {
    vendor: parts[1] || '--',
    part: parts[2] || '--',
  };
}

function updateRamPanel(ram, history, pushHistory) {
  const ramPct = Math.round(ram.used / ram.total * 100);
  const usedGB = (ram.used / 1073741824).toFixed(1);
  const totalGB = Math.round(ram.total / 1073741824);
  const freeGB = (ram.free / 1073741824).toFixed(1);

  pushHistory(history.ram, ramPct);

  document.getElementById('ramUsed').textContent = usedGB;
  document.getElementById('ramTotal').textContent = ` / ${totalGB} GB`;
  document.getElementById('ramFree').textContent = `${freeGB} GB`;
  const ramTempEl = document.getElementById('ramTemp');
  ramTempEl.textContent = ram.temp > 0 ? `${ram.temp.toFixed(0)}°C` : '--°C';
  ramTempEl.style.color = ram.temp > 0 ? resolveTempColor(ram.temp, 70, 85) : '';
  document.getElementById('ramSpeed').textContent = parseRamSpeed(ram.spec, ram.details);
  document.getElementById('ramType').textContent = parseRamType(ram.spec, ram.details);
  document.getElementById('ramDimms').textContent = parseRamDimms(ram.spec, ram.details);

  const details = parseRamDetails(ram.details);
  document.getElementById('ramVendor').textContent = details.vendor;
  document.getElementById('ramPart').textContent = details.part;

  document.getElementById('ramBar').style.width = `${ramPct}%`;
  document.getElementById('ramBarPct').textContent = `${ramPct}%`;
}

export { updateRamPanel };
