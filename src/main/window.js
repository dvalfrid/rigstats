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

module.exports = {
  createMainWindow,
  findDashboardDisplay
};
