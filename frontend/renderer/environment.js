// Environment bridge.
// Exposes a single backend surface to renderer modules.

function getTauriGlobal() {
  return typeof window !== 'undefined' ? window.__TAURI__ || null : null;
}

const tauriGlobal = getTauriGlobal();
const tauriCore = tauriGlobal ? (tauriGlobal.core || tauriGlobal.tauri || tauriGlobal) : null;
const tauriInvoke = tauriCore ? (tauriCore.invoke || null) : null;
const tauriListen = tauriGlobal ? tauriGlobal.event?.listen : null;

const IS_TAURI = typeof tauriInvoke === 'function';
const IS_DESKTOP = IS_TAURI;

function normalizeInvokeChannel(channel) {
  return String(channel).replace(/-/g, '_');
}

const backend = {
  async invoke(channel, payload = {}) {
    if (IS_TAURI) return tauriInvoke(normalizeInvokeChannel(channel), payload);
    throw new Error('Tauri backend is not available');
  },
  on(channel, listener) {
    if (IS_TAURI && typeof tauriListen === 'function') {
      return tauriListen(channel, (event) => listener(null, event.payload));
    }

    return Promise.resolve(() => {});
  },
  async openUrl(url) {
    if (IS_TAURI) {
      const opener = tauriGlobal?.opener;
      if (opener?.openUrl) return opener.openUrl(url);
    }
    window.open(url, '_blank');
  },
};

export { backend, IS_DESKTOP, IS_TAURI };
