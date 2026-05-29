import { describe, it, expect } from 'vitest';
import { parseRamType, parseRamSpeed } from './ram.js';

describe('parseRamType', () => {
  // Regression: old regex /(DDR\d*)/i matched "DDR5" inside "LPDDR5", returning "DDR5".
  it('returns LPDDR5 not DDR5 for LPDDR5 spec string', () => {
    expect(parseRamType('LPDDR5 8000 MT/s (4 DIMMs)', '')).toBe('LPDDR5');
  });

  it('returns LPDDR5X for LPDDR5X', () => {
    expect(parseRamType('LPDDR5X 8533 MT/s (1 DIMMs)', '')).toBe('LPDDR5X');
  });

  it('returns LPDDR4 for LPDDR4', () => {
    expect(parseRamType('LPDDR4 4266 MT/s (2 DIMMs)', '')).toBe('LPDDR4');
  });

  it('returns DDR5 for standard DDR5', () => {
    expect(parseRamType('DDR5 6000 MT/s (2 DIMMs)', '')).toBe('DDR5');
  });

  it('returns DDR4 for standard DDR4', () => {
    expect(parseRamType('DDR4 3200 MT/s (2 DIMMs)', '')).toBe('DDR4');
  });

  it('returns DDR3 for standard DDR3', () => {
    expect(parseRamType('DDR3 1600 MT/s (2 DIMMs)', '')).toBe('DDR3');
  });

  it('returns -- when spec is RAM with no type info', () => {
    expect(parseRamType('RAM (4 DIMMs)', '')).toBe('--');
  });

  it('falls back to details string when spec has no type', () => {
    expect(parseRamType('', '2x16 GB | Samsung | DDR5-6000')).toBe('DDR5');
  });
});

describe('parseRamSpeed', () => {
  it('extracts speed from LPDDR5 spec string', () => {
    expect(parseRamSpeed('LPDDR5 8000 MT/s (4 DIMMs)', '')).toBe('8000 MT/S');
  });

  it('extracts speed from DDR5 spec string', () => {
    expect(parseRamSpeed('DDR5 6000 MT/s (2 DIMMs)', '')).toBe('6000 MT/S');
  });

  it('returns -- when spec has no speed', () => {
    expect(parseRamSpeed('LPDDR5 (4 DIMMs)', '')).toBe('--');
  });

  it('returns -- when spec is just RAM fallback', () => {
    expect(parseRamSpeed('RAM (4 DIMMs)', '')).toBe('--');
  });

  it('infers speed from part number in details as fallback', () => {
    const result = parseRamSpeed('RAM (2 DIMMs)', 'UD5-6000');
    expect(result).toBe('6000 MT/S');
  });
});
