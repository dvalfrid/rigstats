// Pure mapping helpers for CPU/GPU badges and board logos.

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

function resolveRigLogo(brand) {
  const key = String(brand || '').toLowerCase();
  if (key === 'rog') {
    return { src: './assets/ROG_logo_red.png', alt: 'ROG' };
  }
  if (key === 'msi') {
    return { src: './assets/msi.png', alt: 'MSI' };
  }
  if (key === 'gigabyte') {
    return { src: './assets/gigabyte.png', alt: 'Gigabyte' };
  }
  return null;
}

export { resolveVendorBadge, resolveRigLogo };