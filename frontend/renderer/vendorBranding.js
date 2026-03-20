// Pure mapping helpers for CPU/GPU badges and rig brand presentation.

const RIG_BRAND_META = {
  alienware: { label: 'Alienware', src: './assets/Alienware.png', alt: 'Alienware' },
  razer: { label: 'Razer', src: './assets/Razer.png', alt: 'Razer' },
  legion: { label: 'Lenovo Legion', src: './assets/Lenovo-Legion.png', alt: 'Lenovo Legion' },
  omen: { label: 'HP OMEN', src: './assets/HP-Omen.png', alt: 'HP OMEN' },
  predator: { label: 'Acer Predator', src: './assets/Acer-Predator.png', alt: 'Acer Predator' },
  aorus: { label: 'AORUS', src: './assets/AORUS-Gigabyte.png', alt: 'AORUS' },
  rog: { label: 'ROG', src: './assets/ROG_logo_red.png', alt: 'ROG' },
  msi: { label: 'MSI', src: './assets/msi.png', alt: 'MSI' },
  gigabyte: { label: 'Gigabyte', src: './assets/AORUS-Gigabyte.png', alt: 'Gigabyte' },
  asrock: { label: 'ASRock' },
  corsair: { label: 'Corsair' },
  nzxt: { label: 'NZXT' },
  intel: { label: 'Intel' },
  dell: { label: 'Dell' },
  lenovo: { label: 'Lenovo' },
  hp: { label: 'HP' },
  acer: { label: 'Acer' },
};

function resolveVendorBadge(model, kind) {
  const text = (model || '').toLowerCase();
  if (text.includes('nvidia') || text.includes('geforce') || text.includes('rtx') || text.includes('gtx')) {
    return { src: './assets/nvidia.png', alt: `${kind} NVIDIA` };
  }
  if (text.includes('intel') || text.includes('core i') || text.includes('arc')) {
    return { src: './assets/intel.png', alt: `${kind} Intel` };
  }
  if (text.includes('amd') || text.includes('ryzen') || text.includes('radeon')) {
    return { src: './assets/AMD-Radeon-Ryzen-Symbol.png', alt: `${kind} AMD` };
  }
  return null;
}

function normalizeRigBrand(brand) {
  const key = String(brand || '').trim().toLowerCase();
  return RIG_BRAND_META[key] ? key : null;
}

function resolveRigLogo(brand) {
  const key = normalizeRigBrand(brand);
  if (!key) return null;
  const meta = RIG_BRAND_META[key];
  if (!meta.src) return null;
  return { src: meta.src, alt: meta.alt };
}

function resolveRigBrandLabel(brand) {
  const key = normalizeRigBrand(brand);
  if (!key) return null;
  return RIG_BRAND_META[key].label;
}

function resolveArchLogo(cpuModel) {
  const text = (cpuModel || '').toLowerCase();
  if (text.includes('intel') || text.includes('core i') || text.includes('xeon') || text.includes('arc')) {
    return { src: './assets/intel.png', alt: 'Intel' };
  }
  if (text.includes('amd') || text.includes('ryzen') || text.includes('athlon') || text.includes('epyc')) {
    return { src: './assets/AMD-Radeon-Ryzen-Symbol.png', alt: 'AMD' };
  }
  return null;
}

export { normalizeRigBrand, resolveVendorBadge, resolveRigLogo, resolveRigBrandLabel, resolveArchLogo };
