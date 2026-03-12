// src/preload.js
// Exponerar ett säkert API till renderer-processen
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  getStats:       ()      => ipcRenderer.invoke('get-stats'),
  setAutostart:   (bool)  => ipcRenderer.invoke('set-autostart', bool),
  getAutostart:   ()      => ipcRenderer.invoke('get-autostart')
});
