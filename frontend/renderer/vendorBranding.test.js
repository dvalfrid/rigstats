import { describe, expect, it } from 'vitest';

import { resolveRigLogo, resolveVendorBadge } from './vendorBranding.js';

describe('resolveVendorBadge', () => {
  it('maps NVIDIA model strings', () => {
    expect(resolveVendorBadge('NVIDIA GeForce RTX 4080', 'GPU')).toEqual({
      src: './assets/nvidia.png',
      alt: 'GPU NVIDIA'
    });
  });

  it('maps Intel model strings', () => {
    expect(resolveVendorBadge('Intel Core i9-14900K', 'CPU')).toEqual({
      src: './assets/intel.png',
      alt: 'CPU Intel'
    });
  });

  it('maps AMD model strings', () => {
    expect(resolveVendorBadge('AMD Ryzen 9 7950X', 'CPU')).toEqual({
      src: './assets/AMD-Radeon-Ryzen-Symbol.png',
      alt: 'CPU AMD'
    });
  });

  it('returns null for unknown models', () => {
    expect(resolveVendorBadge('Some Unknown Adapter', 'GPU')).toBeNull();
  });
});

describe('resolveRigLogo', () => {
  it('maps rog brand', () => {
    expect(resolveRigLogo('rog')).toEqual({
      src: './assets/ROG_logo_red.png',
      alt: 'ROG'
    });
  });

  it('maps msi brand', () => {
    expect(resolveRigLogo('msi')).toEqual({
      src: './assets/msi.png',
      alt: 'MSI'
    });
  });

  it('maps gigabyte brand', () => {
    expect(resolveRigLogo('gigabyte')).toEqual({
      src: './assets/gigabyte.png',
      alt: 'Gigabyte'
    });
  });

  it('returns null for unsupported brand', () => {
    expect(resolveRigLogo('asrock')).toBeNull();
  });
});