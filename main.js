const { app, BrowserWindow, screen, ipcMain, powerSaveBlocker, Tray, Menu, nativeImage } = require('electron');
const os = require('os');
const si = require('systeminformation');

app.name = 'RigStats';

const { createMainWindow, createSettingsWindow } = require('./src/main/window');
const { loadSettings, saveSettings } = require('./src/main/settings');
const { createTray } = require('./src/main/tray');
const { getStats } = require('./src/main/stats');
const { registerIpcHandlers } = require('./src/main/ipc');

let mainWindow = null;
let settingsWindow = null;
let tray = null;
let currentSettings = null;
let powerBlockerId = null;
let isQuitting = false;

function canCloseWindow() {
  return isQuitting;
}

function onQuitRequested() {
  isQuitting = true;
  app.quit();
}

function buildMainWindow() {
  mainWindow = createMainWindow(BrowserWindow, screen, {
    onCloseAttempt: canCloseWindow
  });
  return mainWindow;
}

function openSettingsWindow(trayBounds) {
  if (settingsWindow && !settingsWindow.isDestroyed()) {
    settingsWindow.focus();
    return;
  }
  settingsWindow = createSettingsWindow(BrowserWindow, trayBounds);
  settingsWindow.on('closed', () => { settingsWindow = null; });
}

app.whenReady().then(() => {
  currentSettings = loadSettings(app);

  if (process.platform === 'win32') {
    app.setAppUserModelId('se.codeby.rigstats');
  }

  tray = createTray(Tray, Menu, nativeImage, {
    getMainWindow: () => mainWindow,
    onQuit: onQuitRequested,
    onOpenSettings: (trayBounds) => openSettingsWindow(trayBounds)
  });
  buildMainWindow();

  registerIpcHandlers(ipcMain, app, os, si, getStats);

  ipcMain.handle('get-opacity', () => currentSettings.opacity);
  ipcMain.on('set-opacity', (_, value) => {
    currentSettings.opacity = value;
    saveSettings(app, currentSettings);
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send('apply-opacity', value);
    }
  });

  powerBlockerId = powerSaveBlocker.start('prevent-app-suspension');
  console.log('✓ App suspension blocked (display sleep allowed)');
});

app.on('before-quit', () => {
  isQuitting = true;
});

app.on('window-all-closed', () => {
  if (powerBlockerId !== null) {
    powerSaveBlocker.stop(powerBlockerId);
    console.log('✓ App suspension block stopped');
  }
  if (process.platform !== 'darwin') app.quit();
});

app.on('activate', () => {
  if (mainWindow) {
    mainWindow.show();
    mainWindow.focus();
  } else {
    buildMainWindow();
  }
});