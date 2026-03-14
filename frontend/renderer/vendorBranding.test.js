import { describe, expect, it } from 'vitest';

import { normalizeRigBrand, resolveArchLogo, resolveRigBrandLabel, resolveRigLogo, resolveVendorBadge } from './vendorBranding.js';

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
      src: './assets/AORUS-Gigabyte.png',
      alt: 'Gigabyte'
    });
  });

  it('maps alienware brand', () => {
    expect(resolveRigLogo('alienware')).toEqual({
      src: './assets/Alienware.png',
      alt: 'Alienware'
    });
  });

  it('maps razer brand', () => {
    expect(resolveRigLogo('razer')).toEqual({
      src: './assets/Razer.png',
      alt: 'Razer'
    });
  });

  it('maps legion brand', () => {
    expect(resolveRigLogo('legion')).toEqual({
      src: './assets/Lenovo-Legion.png',
      alt: 'Lenovo Legion'
    });
  });

  it('maps omen brand', () => {
    expect(resolveRigLogo('omen')).toEqual({
      src: './assets/HP-Omen.png',
      alt: 'HP OMEN'
    });
  });

  it('maps predator brand', () => {
    expect(resolveRigLogo('predator')).toEqual({
      src: './assets/Acer-Predator.png',
      alt: 'Acer Predator'
    });
  });

  it('maps aorus brand', () => {
    expect(resolveRigLogo('aorus')).toEqual({
      src: './assets/AORUS-Gigabyte.png',
      alt: 'AORUS'
    });
  });

  it('returns null for unsupported brand', () => {
    expect(resolveRigLogo('asrock')).toBeNull();
  });
});

describe('normalizeRigBrand', () => {
  it('normalizes known keys', () => {
    expect(normalizeRigBrand(' Alienware ')).toBe('alienware');
    expect(normalizeRigBrand('OMEN')).toBe('omen');
  });

  it('returns null for unknown keys', () => {
    expect(normalizeRigBrand('some-random-oem')).toBeNull();
  });
});

describe('resolveRigBrandLabel', () => {
  it('returns labels for newly supported brands', () => {
    expect(resolveRigBrandLabel('alienware')).toBe('Alienware');
    expect(resolveRigBrandLabel('legion')).toBe('Lenovo Legion');
    expect(resolveRigBrandLabel('omen')).toBe('HP OMEN');
    expect(resolveRigBrandLabel('predator')).toBe('Acer Predator');
    expect(resolveRigBrandLabel('aorus')).toBe('AORUS');
  });

  it('returns current labels for supported logo brands', () => {
    expect(resolveRigBrandLabel('rog')).toBe('ROG');
    expect(resolveRigBrandLabel('msi')).toBe('MSI');
  });
});

describe('resolveArchLogo', () => {
  it('maps Intel CPU model strings', () => {
    expect(resolveArchLogo('Intel Core i9-14900K')).toEqual({ src: './assets/intel.png', alt: 'Intel' });
    expect(resolveArchLogo('Intel Core i7-13700H')).toEqual({ src: './assets/intel.png', alt: 'Intel' });
    expect(resolveArchLogo('Intel Xeon W-2295')).toEqual({ src: './assets/intel.png', alt: 'Intel' });
  });

  it('maps AMD CPU model strings', () => {
    expect(resolveArchLogo('AMD Ryzen 9 7950X')).toEqual({ src: './assets/AMD-Radeon-Ryzen-Symbol.png', alt: 'AMD' });
    expect(resolveArchLogo('AMD Ryzen 7 5800X')).toEqual({ src: './assets/AMD-Radeon-Ryzen-Symbol.png', alt: 'AMD' });
    expect(resolveArchLogo('AMD EPYC 7763')).toEqual({ src: './assets/AMD-Radeon-Ryzen-Symbol.png', alt: 'AMD' });
  });

  it('returns null for unrecognized CPU strings', () => {
    expect(resolveArchLogo('Unknown CPU')).toBeNull();
    expect(resolveArchLogo('')).toBeNull();
    expect(resolveArchLogo(null)).toBeNull();
  });
});