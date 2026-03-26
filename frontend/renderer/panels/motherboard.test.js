import { describe, it, expect } from 'vitest';
import { shortLabel } from './motherboard.js';

describe('shortLabel', () => {
  it('maps "Temperature #N" to "TN"', () => {
    expect(shortLabel('Temperature #1', 8)).toBe('T1');
    expect(shortLabel('Temperature #6', 8)).toBe('T6');
    expect(shortLabel('Temperature #12', 8)).toBe('T12');
  });

  it('is case-insensitive for the Temperature pattern', () => {
    expect(shortLabel('temperature #3', 8)).toBe('T3');
    expect(shortLabel('TEMPERATURE #4', 8)).toBe('T4');
  });

  it('trims whitespace before matching Temperature pattern', () => {
    expect(shortLabel('  Temperature #2  ', 8)).toBe('T2');
  });

  it('returns labels at or below maxLen unchanged', () => {
    expect(shortLabel('Vcore', 8)).toBe('Vcore');
    expect(shortLabel('CPU Core', 8)).toBe('CPU Core'); // exactly 8 chars
    expect(shortLabel('+3.3V', 8)).toBe('+3.3V');
    expect(shortLabel('Fan #7', 6)).toBe('Fan #7');    // exactly 6 chars
  });

  it('truncates labels longer than maxLen', () => {
    expect(shortLabel('CPU Core T', 8)).toBe('CPU Core');
    expect(shortLabel('Fan Channel', 6)).toBe('Fan Ch');
  });

  it('trims whitespace from non-Temperature labels', () => {
    expect(shortLabel('  Vcore  ', 8)).toBe('Vcore');
    expect(shortLabel('  Fan #1  ', 6)).toBe('Fan #1');
  });

  it('returns empty string for an empty or whitespace-only name', () => {
    expect(shortLabel('', 8)).toBe('');
    expect(shortLabel('   ', 8)).toBe('');
  });
});
