// Built-in colour theme presets.
// Each preset defines the primary accent colour; all derived tints (borders,
// backgrounds, scrollbar, stat-label, text-muted) are computed from it so a
// single value drives the entire accent palette.

const PRESETS = {
  'dark-cyan': { accent: '#00c8ff' },
  'amber':     { accent: '#ffb300' },
  'green':     { accent: '#39ff88' },
  'purple':    { accent: '#bf7fff' },
  'slate':     { accent: '#90aac4' },
};

export const VALID_THEME_KEYS = Object.keys(PRESETS);

export function hexToRgba(hex, alpha) {
  const r = parseInt(hex.slice(1, 3), 16);
  const g = parseInt(hex.slice(3, 5), 16);
  const b = parseInt(hex.slice(5, 7), 16);
  return `rgba(${r},${g},${b},${alpha})`;
}

export function hexToHsl(hex) {
  const r = parseInt(hex.slice(1, 3), 16) / 255;
  const g = parseInt(hex.slice(3, 5), 16) / 255;
  const b = parseInt(hex.slice(5, 7), 16) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  let h = 0;
  let s = 0;
  const l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    switch (max) {
      case r: h = ((g - b) / d + (g < b ? 6 : 0)) / 6; break;
      case g: h = ((b - r) / d + 2) / 6; break;
      default: h = ((r - g) / d + 4) / 6;
    }
  }
  return [h * 360, s * 100, l * 100];
}

function hue2rgb(p, q, t) {
  let tt = t;
  if (tt < 0) tt += 1;
  if (tt > 1) tt -= 1;
  if (tt < 1 / 6) return p + (q - p) * 6 * tt;
  if (tt < 1 / 2) return q;
  if (tt < 2 / 3) return p + (q - p) * (2 / 3 - tt) * 6;
  return p;
}

export function hslToHex(h, s, l) {
  const hn = h / 360;
  const sn = s / 100;
  const ln = l / 100;
  let r;
  let g;
  let b;
  if (sn === 0) {
    r = ln;
    g = ln;
    b = ln;
  } else {
    const q = ln < 0.5 ? ln * (1 + sn) : ln + sn - ln * sn;
    const p = 2 * ln - q;
    r = hue2rgb(p, q, hn + 1 / 3);
    g = hue2rgb(p, q, hn);
    b = hue2rgb(p, q, hn - 1 / 3);
  }
  return '#' + [r, g, b].map((x) => Math.round(x * 255).toString(16).padStart(2, '0')).join('');
}

/** Apply a theme preset by key, falling back to dark-cyan if unknown. */
export function applyTheme(key) {
  const preset = PRESETS[key] || PRESETS['dark-cyan'];
  const { accent } = preset;
  const root = document.documentElement.style;

  root.setProperty('--accent',           accent);
  root.setProperty('--accent-border',    hexToRgba(accent, 0.35));
  root.setProperty('--accent-bg',        hexToRgba(accent, 0.12));
  root.setProperty('--accent-bg-thin',   hexToRgba(accent, 0.08));
  root.setProperty('--accent-scrollbar', hexToRgba(accent, 0.45));
  root.setProperty('--accent-grid',      hexToRgba(accent, 0.018));

  // Derive muted label colours from the accent hue so section headings and
  // meta-key labels stay tonally consistent with the active theme.
  const [h] = hexToHsl(accent);
  root.setProperty('--stat-label', hslToHex(h, 32, 64));  // section headers e.g. "CPU LOAD"
  root.setProperty('--text-muted', hslToHex(h, 20, 55));  // meta-key labels e.g. "TEMP", "FREQ"
  root.setProperty('--mb-accent',  hslToHex(h, 40, 52));  // motherboard col headers FANS/TEMPS/VOLTAGES
}
