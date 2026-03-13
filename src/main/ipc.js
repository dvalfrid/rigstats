async function getGpuInfo(si) {
  const gpus = await si.graphics();
  const gpu = (gpus.controllers || []).find((g) => g.vram > 8000) || gpus.controllers?.[0];
  return gpu ? gpu.model : null;
}

function registerIpcHandlers(ipcMain, app, os, si, getStats) {
  ipcMain.handle('get-stats', getStats);

  ipcMain.handle('set-autostart', (_, enable) => {
    app.setLoginItemSettings({ openAtLogin: enable, openAsHidden: false, name: 'RigStats' });
  });

  ipcMain.handle('get-autostart', () => app.getLoginItemSettings().openAtLogin);
  ipcMain.handle('get-system-name', () => os.hostname());

  ipcMain.handle('get-cpu-info', async () => {
    const cpu = await si.cpu();
    return `${cpu.manufacturer} ${cpu.brand}`.trim();
  });

  ipcMain.handle('get-gpu-info', () => getGpuInfo(si));
}

module.exports = {
  registerIpcHandlers
};
