import { describe, it, expect } from 'vitest';
import { truncateName, formatRam } from './process.js';

describe('truncateName', () => {
  it('strips .exe suffix', () => {
    expect(truncateName('chrome.exe')).toBe('chrome');
    expect(truncateName('Discord.exe')).toBe('Discord');
  });

  it('is case-insensitive for .exe suffix', () => {
    expect(truncateName('GAME.EXE')).toBe('GAME');
    expect(truncateName('tool.Exe')).toBe('tool');
  });

  it('does not strip non-.exe extensions', () => {
    expect(truncateName('helper.dll')).toBe('helper.dll');
    expect(truncateName('script.sh')).toBe('script.sh');
  });

  it('returns names at or below MAX_NAME_LEN unchanged after stripping', () => {
    expect(truncateName('chrome.exe')).toBe('chrome'); // 6 chars
    expect(truncateName('abcdefghijklmnop')).toBe('abcdefghijklmnop'); // exactly 16
  });

  it('truncates names longer than MAX_NAME_LEN after stripping', () => {
    expect(truncateName('abcdefghijklmnopq')).toBe('abcdefghijklmnop'); // 17 → 16
    expect(truncateName('ThisIsAVeryLongProcessNameWithoutExe')).toBe('ThisIsAVeryLongP');
  });

  it('truncates the stripped name, not the original', () => {
    // "SomeLongProcess.exe" stripped = "SomeLongProcess" (15 chars) → fits in 16
    expect(truncateName('SomeLongProcess.exe')).toBe('SomeLongProcess');
    // "SomeLongProcessName.exe" stripped = "SomeLongProcessName" (19 chars) → truncate to 16
    expect(truncateName('SomeLongProcessName.exe')).toBe('SomeLongProcessN');
  });

  it('handles names with no suffix or content', () => {
    expect(truncateName('')).toBe('');
    expect(truncateName('svchost')).toBe('svchost');
  });
});

describe('formatRam', () => {
  it('returns MB for values below 1024', () => {
    expect(formatRam(0)).toBe('0M');
    expect(formatRam(512)).toBe('512M');
    expect(formatRam(1023)).toBe('1023M');
  });

  it('returns GB with one decimal for values at or above 1024', () => {
    expect(formatRam(1024)).toBe('1.0G');
    expect(formatRam(2048)).toBe('2.0G');
    expect(formatRam(1536)).toBe('1.5G');
    expect(formatRam(3400)).toBe('3.3G');
  });

  it('rounds GB to one decimal place', () => {
    expect(formatRam(1126)).toBe('1.1G'); // 1126/1024 ≈ 1.099 → 1.1
    expect(formatRam(10240)).toBe('10.0G');
  });
});
