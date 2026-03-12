let ipcRenderer = null;

try {
  if (window.require) {
    ipcRenderer = window.require('electron').ipcRenderer;
  }
} catch (e) {
  ipcRenderer = null;
}

const IS_ELECTRON = ipcRenderer !== null;

export { ipcRenderer, IS_ELECTRON };
