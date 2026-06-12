import { describe, expect, it } from 'vitest';
import { readFileSync } from 'fs';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';

const src = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'panel-host.js'), 'utf8');

describe('panel-host Tauri event names', () => {
  // Tauri v2 emits 'tauri://move' (without 'd') for window move events.
  // Using 'tauri://moved' silently registers a listener that never fires,
  // causing panel positions to never be saved when panels are dragged.
  it("listens on 'tauri://move', not 'tauri://moved'", () => {
    expect(src).toContain("'tauri://move'");
    expect(src).not.toContain("'tauri://moved'");
  });
});
