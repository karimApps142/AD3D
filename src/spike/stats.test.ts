import { describe, expect, it } from 'vitest';
import { triangleWave } from './sliderDriver';
import { countPerWindow, intervals, median, percentile, round } from './stats';

describe('spike statistics', () => {
  it('computes medians and interpolated percentiles', () => {
    expect(median([5, 1, 3])).toBe(3);
    expect(median([4, 1, 3, 2])).toBe(2.5);
    expect(percentile([1, 2, 3, 4, 5], 95)).toBeCloseTo(4.8, 9);
    expect(percentile([], 50)).toBeNaN();
    expect(median([Number.NaN, 2, 4])).toBe(3);
  });

  it('rounds and maps non-finite values to null', () => {
    expect(round(1.23456, 2)).toBe(1.23);
    expect(round(Number.NaN)).toBeNull();
    expect(round(Infinity)).toBeNull();
  });

  it('counts events per window', () => {
    expect(countPerWindow([1000, 1500, 1999, 2000, 4999, 5000], 1000, 4)).toEqual([3, 1, 0, 1]);
  });

  it('computes intervals', () => {
    expect(intervals([0, 16, 33, 50])).toEqual([16, 17, 17]);
  });

  it('sweeps a triangle wave', () => {
    expect(triangleWave(0, 2, 18, 1600)).toBe(2);
    expect(triangleWave(800, 2, 18, 1600)).toBe(18);
    expect(triangleWave(400, 2, 18, 1600)).toBe(10);
    expect(triangleWave(1600, 2, 18, 1600)).toBe(2);
  });
});
