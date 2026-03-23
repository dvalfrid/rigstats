import { backend, IS_DESKTOP } from './environment.js';

const ids = {
  heroTitle: document.getElementById('heroTitle'),
  currentVersion: document.getElementById('currentVersion'),
  newVersion: document.getElementById('newVersion'),
  notesContent: document.getElementById('notesContent'),
  progressWrap: document.getElementById('progressWrap'),
  progressLabel: document.getElementById('progressLabel'),
  progressFill: document.getElementById('progressFill'),
  statusMsg: document.getElementById('statusMsg'),
  updateBtn: document.getElementById('updateBtn'),
  laterBtn: document.getElementById('laterBtn'),
};

let isInstalling = false;

function setStatus(msg) {
  ids.statusMsg.textContent = msg;
}

function showProgress(pct, label) {
  ids.progressWrap.classList.add('visible');
  ids.progressFill.style.width = `${pct}%`;
  if (label) ids.progressLabel.textContent = label;
}

async function loadUpdateInfo() {
  if (!IS_DESKTOP) {
    ids.currentVersion.textContent = '1.5.1';
    ids.newVersion.textContent = '2.0.0';
    ids.notesContent.textContent = 'Auto-update support.\nDashboard improvements.';
    return;
  }

  try {
    const info = await backend.invoke('check-for-update');
    if (!info) {
      ids.heroTitle.textContent = 'Up to Date';
      ids.notesContent.textContent = 'No update available.';
      ids.updateBtn.disabled = true;
      setStatus('You are running the latest version.');
      return;
    }
    ids.currentVersion.textContent = `v${info.currentVersion}`;
    ids.newVersion.textContent = `v${info.version}`;
    ids.notesContent.textContent = info.body || 'See GitHub releases for details.';
  } catch (err) {
    ids.notesContent.textContent = 'Could not check for updates.';
    ids.updateBtn.disabled = true;
    setStatus(`Error: ${err}`);
  }
}

async function startUpdate() {
  if (isInstalling) return;
  isInstalling = true;
  ids.updateBtn.disabled = true;
  ids.laterBtn.disabled = true;
  ids.heroTitle.textContent = 'Downloading Update…';
  showProgress(0, 'DOWNLOADING...');
  setStatus('Downloading update — do not close this window.');

  if (!IS_DESKTOP) return;

  const unlistenProgress = await backend.on('update-progress', (_event, data) => {
    if (data.total) {
      const pct = Math.round((data.downloaded / data.total) * 100);
      showProgress(pct, `DOWNLOADING  ${pct}%`);
    } else {
      showProgress(100, 'DOWNLOADING...');
    }
  });

  try {
    await backend.invoke('install-update');
    // If we reach here the installer is running — app may close at any moment.
    showProgress(100, 'INSTALLING...');
    ids.heroTitle.textContent = 'Installing…';
    setStatus('Installing update. The app will restart automatically.');
  } catch (err) {
    isInstalling = false;
    ids.updateBtn.disabled = false;
    ids.laterBtn.disabled = false;
    ids.heroTitle.textContent = 'Update Failed';
    showProgress(0);
    setStatus(`Install failed: ${err}`);
  } finally {
    unlistenProgress();
  }
}

ids.updateBtn.addEventListener('click', startUpdate);
ids.laterBtn.addEventListener('click', async () => {
  if (!IS_DESKTOP) return;
  await backend.invoke('close-window');
});

document.addEventListener('keydown', async (event) => {
  if (event.key === 'Escape' && IS_DESKTOP && !isInstalling) {
    await backend.invoke('close-window');
  }
});

loadUpdateInfo();
