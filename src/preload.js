// Electron preload bridge (legacy mode).
// Exposes a minimal, controlled API surface to the renderer process.
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  getStats:       ()      => ipcRenderer.invoke('get-stats'),
  setAutostart:   (bool)  => ipcRenderer.invoke('set-autostart', bool),
  getAutostart:   ()      => ipcRenderer.invoke('get-autostart')
});
