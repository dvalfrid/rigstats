// Environment bridge.
// Exposes a unified backend API for both Electron legacy mode and Tauri mode.

let electronIpc = null;

try {
  if (window.require) {
    electronIpc = window.require('electron').ipcRenderer;
  }
} catch (_e) {
  electronIpc = null;
}

const tauriGlobal = window.__TAURI__ || null;
const tauriInvoke = tauriGlobal ? (tauriGlobal.invoke || tauriGlobal.tauri?.invoke) : null;
const tauriListen = tauriGlobal ? tauriGlobal.event?.listen : null;

const IS_ELECTRON = electronIpc !== null;
const IS_TAURI = typeof tauriInvoke === 'function';
const IS_DESKTOP = IS_ELECTRON || IS_TAURI;

function normalizeInvokeChannel(channel) {
  // Tauri command names are snake_case while renderer calls use kebab-case.
  if (!IS_TAURI) return channel;
  return String(channel).replace(/-/g, '_');
}

const backend = {
  async invoke(channel, payload = {}) {
    if (IS_ELECTRON) return electronIpc.invoke(channel, payload);
    if (IS_TAURI) return tauriInvoke(normalizeInvokeChannel(channel), payload);
    throw new Error('No desktop backend available');
  },
  on(channel, listener) {
    if (IS_ELECTRON) return electronIpc.on(channel, listener);
    if (IS_TAURI && typeof tauriListen === 'function') {
      tauriListen(channel, (event) => listener(null, event.payload));
    }
  }
};

export { backend, IS_DESKTOP, IS_ELECTRON, IS_TAURI };
