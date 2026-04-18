import { describe, expect, it } from 'vitest';

import {
  buildGpuPreferencePayload,
  buildSelectorModel,
  renderSelectorButtons,
  selectorFallbackMarkup,
} from './gpu.js';

describe('buildSelectorModel', () => {
  it('returns none state for missing GPU list', () => {
    const model = buildSelectorModel({ name: 'GPU A' });
    expect(model.state).toBe('none');
    expect(model.options).toHaveLength(0);
  });

  it('returns single state for one GPU', () => {
    const model = buildSelectorModel({
      name: 'GPU A',
      availableGpus: [['GPU A', 8192]],
    });
    expect(model.state).toBe('single');
    expect(model.options).toHaveLength(1);
    expect(model.options[0]).toMatchObject({ name: 'GPU A', vramMb: 8192, selected: true });
  });

  it('returns multi state and marks selected GPU', () => {
    const model = buildSelectorModel({
      name: 'GPU B',
      availableGpus: [['GPU A', 4096], ['GPU B', 12288]],
    });
    expect(model.state).toBe('multi');
    expect(model.options).toHaveLength(2);
    expect(model.options[0].selected).toBe(false);
    expect(model.options[1].selected).toBe(true);
  });

  it('filters malformed entries and normalizes invalid VRAM values', () => {
    const model = buildSelectorModel({
      name: 'GPU A',
      availableGpus: [null, ['GPU A', Number.NaN], ['GPU B']],
    });
    expect(model.options).toHaveLength(2);
    expect(model.options[0].vramMb).toBe(0);
    expect(model.options[1].vramMb).toBe(0);
  });

  it('changes cache key when selected GPU changes', () => {
    const a = buildSelectorModel({
      name: 'GPU A',
      availableGpus: [['GPU A', 8192], ['GPU B', 4096]],
    });
    const b = buildSelectorModel({
      name: 'GPU B',
      availableGpus: [['GPU A', 8192], ['GPU B', 4096]],
    });
    expect(a.key).not.toBe(b.key);
  });
});

describe('selectorFallbackMarkup', () => {
  it('returns 1 GPU text for single state', () => {
    expect(selectorFallbackMarkup('single')).toContain('1 GPU');
  });

  it('returns AUTO text for none state', () => {
    expect(selectorFallbackMarkup('none')).toContain('AUTO');
  });
});

describe('renderSelectorButtons', () => {
  it('renders one button per GPU option', () => {
    const html = renderSelectorButtons([
      { name: 'GPU A', vramMb: 8192, selected: true },
      { name: 'GPU B', vramMb: 4096, selected: false },
    ]);
    expect((html.match(/<button/g) || []).length).toBe(2);
  });

  it('escapes unsafe characters in attributes', () => {
    const html = renderSelectorButtons([
      { name: 'GPU "A" <script>', vramMb: 1024, selected: false },
    ]);
    expect(html).toContain('&quot;A&quot;');
    expect(html).toContain('&lt;script&gt;');
  });

  it('uses highlighted styles for selected option', () => {
    const html = renderSelectorButtons([
      { name: 'GPU A', vramMb: 8192, selected: true },
    ]);
    expect(html).toContain('background:var(--amd)');
    expect(html).toContain('box-shadow:0 0 6px rgba(255,58,31,0.65)');
  });
});

describe('buildGpuPreferencePayload', () => {
  it('returns both snake_case and camelCase keys for command compatibility', () => {
    const payload = buildGpuPreferencePayload('NVIDIA GeForce RTX 9070 XT');
    expect(payload).toEqual({
      gpu_name: 'NVIDIA GeForce RTX 9070 XT',
      gpuName: 'NVIDIA GeForce RTX 9070 XT',
    });
  });
});
