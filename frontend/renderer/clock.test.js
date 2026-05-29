import { describe, it, expect } from 'vitest';
import { formatUptime } from './clock.js';

describe('formatUptime', () => {
  it('formats zero as UP 00:00:00', () => {
    expect(formatUptime(0)).toBe('UP 00:00:00');
  });

  it('zero-pads single-digit values', () => {
    expect(formatUptime(61)).toBe('UP 00:01:01');
  });

  it('formats hours, minutes, seconds correctly', () => {
    expect(formatUptime(3661)).toBe('UP 01:01:01');
    expect(formatUptime(86399)).toBe('UP 23:59:59');
    expect(formatUptime(90000)).toBe('UP 25:00:00');
  });

  it('floors fractional seconds', () => {
    expect(formatUptime(59.9)).toBe('UP 00:00:59');
  });

  it('returns UP 00:00:00 for non-finite input', () => {
    expect(formatUptime(NaN)).toBe('UP 00:00:00');
    expect(formatUptime(Infinity)).toBe('UP 00:00:00');
    expect(formatUptime(undefined)).toBe('UP 00:00:00');
  });

  it('clamps negative values to zero', () => {
    expect(formatUptime(-5)).toBe('UP 00:00:00');
  });
});
