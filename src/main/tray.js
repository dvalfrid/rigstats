const fs = require('fs');
const path = require('path');

function createTray(Tray, Menu, nativeImage, options = {}) {
  const {
    getMainWindow,
    onQuit,
    onOpenSettings
  } = options;

  const trayPngPath = path.join(__dirname, '..', '..', 'assets', 'tray.png');
  const fallbackIcoPath = path.join(__dirname, '..', '..', 'assets', 'icon.ico');
  const iconPath = fs.existsSync(trayPngPath) ? trayPngPath : fallbackIcoPath;

  let trayImage = nativeImage.createFromPath(iconPath);
  if (!trayImage.isEmpty()) {
    trayImage = trayImage.resize({ width: 16, height: 16, quality: 'best' });
  }

  const tray = new Tray(trayImage.isEmpty() ? iconPath : trayImage);
  tray.setToolTip('RigStats');

  const showWindow = () => {
    const mainWindow = typeof getMainWindow === 'function' ? getMainWindow() : null;
    if (!mainWindow) return;
    mainWindow.show();
    mainWindow.focus();
  };

  const toggleWindow = () => {
    const mainWindow = typeof getMainWindow === 'function' ? getMainWindow() : null;
    if (!mainWindow) return;
    if (mainWindow.isVisible()) mainWindow.hide();
    else showWindow();
  };

  const contextMenu = Menu.buildFromTemplate([
    { label: 'Show RigStats', click: showWindow },
    { type: 'separator' },
    {
      label: 'Settings',
      click: () => {
        if (typeof onOpenSettings === 'function') onOpenSettings(tray.getBounds());
      }
    },
    { type: 'separator' },
    {
      label: 'Quit',
      click: () => {
        if (typeof onQuit === 'function') onQuit();
      }
    }
  ]);

  tray.setContextMenu(contextMenu);
  tray.on('click', toggleWindow);

  return tray;
}

module.exports = {
  createTray
};
