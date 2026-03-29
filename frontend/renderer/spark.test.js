import { describe, expect, it } from 'vitest';

import { createHistory, pushHistory } from './spark.js';

describe('createHistory', () => {
  it('creates arrays of the requested length filled with zeros', () => {
    const h = createHistory(10);
    expect(h.cpu).toHaveLength(10);
    expect(h.gpu).toHaveLength(10);
    expect(h.ram).toHaveLength(10);
    expect(h.netDown).toHaveLength(10);
    expect(h.netUp).toHaveLength(10);
    expect(h.diskRead).toHaveLength(10);
    expect(h.diskWrite).toHaveLength(10);
  });

  it('fills all entries with zero', () => {
    const h = createHistory(5);
    expect(h.cpu.every((v) => v === 0)).toBe(true);
    expect(h.diskRead.every((v) => v === 0)).toBe(true);
    expect(h.diskWrite.every((v) => v === 0)).toBe(true);
    expect(h.netDown.every((v) => v === 0)).toBe(true);
    expect(h.netUp.every((v) => v === 0)).toBe(true);
  });

  it('defaults to length 80', () => {
    const h = createHistory();
    expect(h.cpu).toHaveLength(80);
  });

  it('each series is an independent array', () => {
    const h = createHistory(4);
    h.cpu[0] = 99;
    expect(h.gpu[0]).toBe(0);
  });
});

describe('pushHistory', () => {
  it('appends the new value at the end', () => {
    const series = [0, 0, 0];
    pushHistory(series, 42);
    expect(series[2]).toBe(42);
  });

  it('removes the oldest value from the front', () => {
    const series = [1, 2, 3];
    pushHistory(series, 4);
    expect(series[0]).toBe(2);
  });

  it('keeps the array length constant', () => {
    const series = [1, 2, 3, 4, 5];
    pushHistory(series, 99);
    expect(series).toHaveLength(5);
  });

  it('maintains correct order after multiple pushes', () => {
    const series = [0, 0, 0];
    pushHistory(series, 1);
    pushHistory(series, 2);
    pushHistory(series, 3);
    expect(series).toEqual([1, 2, 3]);
  });

  it('oldest values are evicted when the buffer is full', () => {
    const series = [10, 20, 30];
    pushHistory(series, 40);
    pushHistory(series, 50);
    expect(series).toEqual([30, 40, 50]);
  });
});
