const path = require('path');

function findDashboardDisplay(screen) {
  const displays = screen.getAllDisplays();
  console.log('Available displays:');
  displays.forEach((d, i) => {
    console.log(`  [${i}] ${d.size.width}x${d.size.height} @ (${d.bounds.x},${d.bounds.y})`);
  });

  const target = displays.find((d) => d.size.width === 450 && d.size.height === 1920);
  if (target) {
    console.log('Found 450x1920 display');
    return target;
  }

  if (displays.length > 1) {
    console.log('Using secondary display as fallback');
    return displays[1];
  }

  return null;
}

function createMainWindow(BrowserWindow, screen, options = {}) {
  const { onCloseAttempt } = options;
  const targetDisplay = findDashboardDisplay(screen);
  const iconPath = path.join(__dirname, '..', '..', 'assets', 'icon.ico');

  const windowConfig = {
    width: 450,
    height: 1920,
    frame: false,
    resizable: false,
    alwaysOnTop: false,
    skipTaskbar: true,
    transparent: true,
    icon: iconPath,
    webPreferences: { nodeIntegration: true, contextIsolation: false }
  };

  if (targetDisplay) {
    windowConfig.x = targetDisplay.bounds.x;
    windowConfig.y = targetDisplay.bounds.y;
  }

  const mainWindow = new BrowserWindow(windowConfig);
  mainWindow.loadFile(path.join(__dirname, '..', 'index.html'));

  if (targetDisplay) {
    mainWindow.setBounds({
      x: targetDisplay.bounds.x,
      y: targetDisplay.bounds.y,
      width: targetDisplay.size.width,
      height: targetDisplay.size.height
    });
  }

  mainWindow.on('close', (event) => {
    if (typeof onCloseAttempt === 'function') {
      const shouldClose = onCloseAttempt();
      if (!shouldClose) {
        event.preventDefault();
        mainWindow.hide();
      }
    }
  });

  return mainWindow;
}

function createSettingsWindow(BrowserWindow, trayBounds) {
  const { screen } = require('electron');
  const { workArea } = screen.getPrimaryDisplay();
  const winW = 300;
  const winH = 120;

  let x = workArea.x + workArea.width - winW - 16;
  let y = workArea.y + workArea.height - winH - 16;

  if (trayBounds && trayBounds.width > 0) {
    x = Math.round(trayBounds.x + trayBounds.width / 2 - winW / 2);
    y = Math.round(trayBounds.y - winH - 6);
  }

  x = Math.max(workArea.x, Math.min(workArea.x + workArea.width - winW, x));
  y = Math.max(workArea.y, Math.min(workArea.y + workArea.height - winH, y));

  const win = new BrowserWindow({
    width: winW,
    height: winH,
    x,
    y,
    frame: false,
    resizable: false,
    alwaysOnTop: true,
    skipTaskbar: true,
    backgroundColor: '#0b0d12',
    webPreferences: { nodeIntegration: true, contextIsolation: false }
  });

  win.loadFile(path.join(__dirname, '..', 'settings.html'));
  win.on('blur', () => win.close());

  return win;
}

module.exports = {
  createMainWindow,
  createSettingsWindow,
  findDashboardDisplay
};
