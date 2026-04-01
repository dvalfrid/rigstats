import { describe, expect, it } from 'vitest';

import { VALID_THEME_KEYS, hexToHsl, hexToRgba, hslToHex } from './themes.js';

describe('VALID_THEME_KEYS', () => {
  it('contains exactly the five built-in presets', () => {
    expect(VALID_THEME_KEYS).toEqual(['dark-cyan', 'amber', 'green', 'purple', 'slate']);
  });
});

describe('hexToRgba', () => {
  it('converts pure cyan to rgba', () => {
    expect(hexToRgba('#00c8ff', 0.35)).toBe('rgba(0,200,255,0.35)');
  });

  it('converts white correctly', () => {
    expect(hexToRgba('#ffffff', 1)).toBe('rgba(255,255,255,1)');
  });

  it('converts black correctly', () => {
    expect(hexToRgba('#000000', 0)).toBe('rgba(0,0,0,0)');
  });

  it('converts amber correctly', () => {
    expect(hexToRgba('#ffb300', 0.12)).toBe('rgba(255,179,0,0.12)');
  });
});

describe('hexToHsl', () => {
  it('pure red is hue 0°', () => {
    const [h, s, l] = hexToHsl('#ff0000');
    expect(h).toBeCloseTo(0, 0);
    expect(s).toBeCloseTo(100, 0);
    expect(l).toBeCloseTo(50, 0);
  });

  it('pure green is hue 120°', () => {
    const [h] = hexToHsl('#00ff00');
    expect(h).toBeCloseTo(120, 0);
  });

  it('pure blue is hue 240°', () => {
    const [h] = hexToHsl('#0000ff');
    expect(h).toBeCloseTo(240, 0);
  });

  it('white has saturation 0 and lightness 100', () => {
    const [, s, l] = hexToHsl('#ffffff');
    expect(s).toBe(0);
    expect(l).toBeCloseTo(100, 0);
  });

  it('black has saturation 0 and lightness 0', () => {
    const [, s, l] = hexToHsl('#000000');
    expect(s).toBe(0);
    expect(l).toBe(0);
  });

  it('dark-cyan accent lands in the cyan hue range (175°–200°)', () => {
    const [h] = hexToHsl('#00c8ff');
    expect(h).toBeGreaterThan(175);
    expect(h).toBeLessThan(200);
  });

  it('amber accent lands in the yellow-orange hue range (40°–50°)', () => {
    const [h] = hexToHsl('#ffb300');
    expect(h).toBeGreaterThan(40);
    expect(h).toBeLessThan(50);
  });

  it('purple accent lands in the purple hue range (270°–290°)', () => {
    const [h] = hexToHsl('#bf7fff');
    expect(h).toBeGreaterThanOrEqual(270);
    expect(h).toBeLessThan(290);
  });
});

describe('hslToHex', () => {
  it('achromatic (s=0) returns a grey', () => {
    const hex = hslToHex(0, 0, 50);
    // Each channel should be ~128 (0x80)
    const r = parseInt(hex.slice(1, 3), 16);
    const g = parseInt(hex.slice(3, 5), 16);
    const b = parseInt(hex.slice(5, 7), 16);
    expect(r).toBe(g);
    expect(g).toBe(b);
  });

  it('round-trips through hexToHsl → hslToHex within 2 points', () => {
    for (const hex of ['#00c8ff', '#ffb300', '#39ff88', '#bf7fff', '#90aac4']) {
      const [h, s, l] = hexToHsl(hex);
      const roundTripped = hslToHex(h, s, l);
      // Compare channel-by-channel; allow ±2 due to rounding
      for (let i = 0; i < 3; i++) {
        const orig = parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16);
        const back = parseInt(roundTripped.slice(1 + i * 2, 3 + i * 2), 16);
        expect(Math.abs(orig - back)).toBeLessThanOrEqual(2);
      }
    }
  });

  it('derived stat-label has lower saturation than highly-saturated accents', () => {
    // Verify that the derived colours are genuinely muted relative to the accent.
    // Only test accents whose natural saturation exceeds the fixed stat-label
    // saturation (32%) — slate (#90aac4, S≈30%) is already desaturated and is skipped.
    for (const hex of ['#00c8ff', '#ffb300', '#39ff88', '#bf7fff']) {
      const [h, s] = hexToHsl(hex);
      const label = hslToHex(h, 32, 64);
      const [, labelS] = hexToHsl(label);
      expect(labelS).toBeLessThan(s);
    }
  });
});
