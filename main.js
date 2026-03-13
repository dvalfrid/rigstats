const { app, BrowserWindow, screen, ipcMain, powerSaveBlocker, Tray, Menu, nativeImage } = require('electron');
const os = require('os');
const si = require('systeminformation');

app.name = 'RigStats';

const { createMainWindow } = require('./src/main/window');
const { createTray } = require('./src/main/tray');
const { getStats } = require('./src/main/stats');
const { registerIpcHandlers } = require('./src/main/ipc');

let mainWindow = null;
let tray = null;
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

app.whenReady().then(() => {
  if (process.platform === 'win32') {
    app.setAppUserModelId('se.codeby.rigstats');
  }

  tray = createTray(Tray, Menu, nativeImage, {
    getMainWindow: () => mainWindow,
    onQuit: onQuitRequested
  });
  buildMainWindow();

  registerIpcHandlers(ipcMain, app, os, si, getStats);

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