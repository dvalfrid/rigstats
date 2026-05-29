import { describe, it, expect } from 'vitest';
import { batColor, powerColor, formatTimeRemaining } from './battery.js';

describe('batColor', () => {
  it('returns accent color when charging regardless of charge level', () => {
    expect(batColor(5, true)).toBe('var(--accent)');
    expect(batColor(100, true)).toBe('var(--accent)');
  });

  it('returns green when above 50%', () => {
    expect(batColor(51, false)).toBe('var(--grn)');
    expect(batColor(100, false)).toBe('var(--grn)');
  });

  // Boundary: pct > 20 → amber, pct <= 20 → red.
  it('returns amber when above 20% and at most 50%', () => {
    expect(batColor(50, false)).toBe('#ffb300');
    expect(batColor(21, false)).toBe('#ffb300');
    expect(batColor(35, false)).toBe('#ffb300');
  });

  it('returns red at 20% and below', () => {
    expect(batColor(20, false)).toBe('var(--amd)');
    expect(batColor(19, false)).toBe('var(--amd)');
    expect(batColor(0, false)).toBe('var(--amd)');
  });
});

describe('powerColor', () => {
  it('returns empty string when charging', () => {
    expect(powerColor(25, true)).toBe('');
    expect(powerColor(5, true)).toBe('');
  });

  it('returns empty string when power is null', () => {
    expect(powerColor(null, false)).toBe('');
  });

  it('returns green below 12 W', () => {
    expect(powerColor(0, false)).toBe('var(--grn)');
    expect(powerColor(11.9, false)).toBe('var(--grn)');
  });

  it('returns amber between 12 W and 20 W', () => {
    expect(powerColor(12, false)).toBe('#ffb300');
    expect(powerColor(19.9, false)).toBe('#ffb300');
  });

  it('returns red at 20 W and above', () => {
    expect(powerColor(20, false)).toBe('var(--amd)');
    expect(powerColor(65, false)).toBe('var(--amd)');
  });
});

describe('formatTimeRemaining', () => {
  it('returns -- for null or zero', () => {
    expect(formatTimeRemaining(null)).toBe('--');
    expect(formatTimeRemaining(0)).toBe('--');
    expect(formatTimeRemaining(undefined)).toBe('--');
  });

  it('shows only minutes when under one hour', () => {
    expect(formatTimeRemaining(30)).toBe('30m');
    expect(formatTimeRemaining(59)).toBe('59m');
  });

  it('shows hours and minutes when one hour or more', () => {
    expect(formatTimeRemaining(60)).toBe('1h 0m');
    expect(formatTimeRemaining(145)).toBe('2h 25m');
    expect(formatTimeRemaining(90)).toBe('1h 30m');
  });
});
