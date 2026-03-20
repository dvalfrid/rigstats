import { describe, expect, it } from 'vitest';

import {
  TEMP_HOT_COLOR,
  TEMP_OK_COLOR,
  TEMP_UNKNOWN_COLOR,
  TEMP_WARM_COLOR,
  resolveTempColor,
} from './tempColors.js';

describe('resolveTempColor', () => {
  it('returns unknown for missing or non-positive values', () => {
    expect(resolveTempColor(null, 70, 85)).toBe(TEMP_UNKNOWN_COLOR);
    expect(resolveTempColor(undefined, 70, 85)).toBe(TEMP_UNKNOWN_COLOR);
    expect(resolveTempColor(Number.NaN, 70, 85)).toBe(TEMP_UNKNOWN_COLOR);
    expect(resolveTempColor(0, 70, 85)).toBe(TEMP_UNKNOWN_COLOR);
    expect(resolveTempColor(-5, 70, 85)).toBe(TEMP_UNKNOWN_COLOR);
  });

  it('returns green below warm threshold', () => {
    expect(resolveTempColor(45, 70, 85)).toBe(TEMP_OK_COLOR);
    expect(resolveTempColor(69.9, 70, 85)).toBe(TEMP_OK_COLOR);
  });

  it('returns yellow at or above warm threshold and below hot threshold', () => {
    expect(resolveTempColor(70, 70, 85)).toBe(TEMP_WARM_COLOR);
    expect(resolveTempColor(84.9, 70, 85)).toBe(TEMP_WARM_COLOR);
  });

  it('returns red at or above hot threshold', () => {
    expect(resolveTempColor(85, 70, 85)).toBe(TEMP_HOT_COLOR);
    expect(resolveTempColor(101, 70, 85)).toBe(TEMP_HOT_COLOR);
  });
});
