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

// Logs capability/permission errors that would otherwise be silently swallowed.
// Only fires for "not allowed" responses from Tauri's ACL system — transient
// errors (position unavailable, IPC timeout, etc.) are intentionally ignored.
function logPermissionError(err, context) {
  const msg = String(err);
  if (!msg.includes('not allowed') && !msg.toLowerCase().includes('permission denied')) return;
  const line = `[capability-denied] ${context}: ${msg.split('\n')[0]}`;
  console.error(line);
  if (IS_TAURI) tauriInvoke('log_frontend_error', { message: line }).catch(() => {});
}

export { backend, IS_DESKTOP, IS_TAURI, logPermissionError };
