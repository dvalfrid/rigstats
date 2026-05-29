import { backend, IS_DESKTOP } from './environment.js';

const ids = {
  rigstatsVersion: document.getElementById('rigstatsVersion'),
  serviceState: document.getElementById('serviceState'),
  pipeHealth: document.getElementById('pipeHealth'),
  logPath: document.getElementById('logPath'),
  lastSuccessAt: document.getElementById('lastSuccessAt'),
  logTail: document.getElementById('logTail'),
  dependenciesTable: document.getElementById('dependenciesTable'),
  copyState: document.getElementById('copyState'),
  refreshBtn: document.getElementById('refreshBtn'),
  copyLogBtn: document.getElementById('copyLogBtn'),
  closeBtn: document.getElementById('closeBtn'),
  collectDiagBtn: document.getElementById('collectDiagBtn'),
  diagState: document.getElementById('diagState'),
};

let currentInfo = null;
let autoRefreshTimer = null;
let hasRecordedSuccessfulRefresh = false;

function setCopyState(message) {
  ids.copyState.textContent = message;
  window.clearTimeout(setCopyState.timer);
  setCopyState.timer = window.setTimeout(() => {
    ids.copyState.textContent = '';
  }, 1800);
}

function formatLocalTimestamp(date) {
  return date.toLocaleString('sv-SE', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

function setSidecarStatus(info) {
  const state = (info.sidecarServiceState || '').trim().toUpperCase();

  let serviceClass = 'health-neutral';
  if (state === 'RUNNING') serviceClass = 'health-good';
  else if (state === 'STOPPED' || state === 'FAILED') serviceClass = 'health-bad';
  ids.serviceState.textContent = state || 'UNKNOWN';
  ids.serviceState.className = `meta-value ${serviceClass}`;

  if (info.sidecarPipeConnected) {
    ids.pipeHealth.textContent = 'Connected';
    ids.pipeHealth.className = 'meta-value health-good';
    ids.pipeHealth.title = 'The sidecar pipe has delivered at least one sensor payload since startup.';
  } else {
    ids.pipeHealth.textContent = 'No data';
    ids.pipeHealth.className = 'meta-value health-bad';
    ids.pipeHealth.title = 'No sensor payload received from the sidecar pipe since startup. Check that the rigstats-sensor service is running.';
  }
}

function renderDependencies(items) {
  ids.dependenciesTable.innerHTML = items.map((item) => `
    <tr>
      <td>
        <div class="dep-name">${item.name}</div>
        <div class="dep-note">${item.note || ''}</div>
      </td>
      <td class="dep-version">${item.version}</td>
      <td class="dep-status">
        <span class="dep-status-badge ${item.ok ? 'ok' : 'fail'}">${item.status}</span>
      </td>
    </tr>
  `).join('');
}

function render(info) {
  currentInfo = info;
  const shouldStickToBottom = ids.logTail.scrollHeight - ids.logTail.scrollTop - ids.logTail.clientHeight < 24;

  ids.rigstatsVersion.textContent = info.rigstatsVersion;
  setSidecarStatus(info);
  ids.logPath.textContent = info.logPath;
  ids.logTail.value = info.logTail || '(empty log)';
  if (shouldStickToBottom) {
    ids.logTail.scrollTop = ids.logTail.scrollHeight;
  }
  renderDependencies(info.dependencies || []);
}

async function refresh(options = {}) {
  const { markManual = false } = options;

  if (!IS_DESKTOP) {
    ids.logTail.value = 'Tauri backend unavailable.';
    return;
  }

  try {
    render(await backend.invoke('get-about-info'));
    if (markManual || !hasRecordedSuccessfulRefresh) {
      ids.lastSuccessAt.textContent = formatLocalTimestamp(new Date());
      hasRecordedSuccessfulRefresh = true;
    }
  } catch (error) {
    console.error('get-about-info failed:', error);
    ids.logTail.value = `Could not load status data.\n\n${String(error)}`;
  }
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
    setCopyState('Copied');
  } catch (error) {
    console.error('clipboard write failed:', error);
    setCopyState('Copy failed');
  }
}

ids.refreshBtn.addEventListener('click', () => refresh({ markManual: true }));
ids.copyLogBtn.addEventListener('click', () => copyText(currentInfo?.logTail || ''));
ids.closeBtn.addEventListener('click', async () => {
  if (!IS_DESKTOP) return;
  await backend.invoke('close-window');
});

ids.collectDiagBtn.addEventListener('click', async () => {
  if (!IS_DESKTOP) return;

  ids.collectDiagBtn.disabled = true;
  ids.diagState.textContent = 'Collecting…';

  try {
    const savedPath = await backend.invoke('collect-diagnostics');
    if (savedPath == null) {
      ids.diagState.textContent = 'Cancelled';
    } else {
      ids.diagState.textContent = `Saved to: ${savedPath}`;
    }
  } catch (error) {
    console.error('collect-diagnostics failed:', error);
    ids.diagState.textContent = `Error: ${error?.message ?? error}`;
  } finally {
    ids.collectDiagBtn.disabled = false;
    window.setTimeout(() => { ids.diagState.textContent = ''; }, 6000);
  }
});

function startAutoRefresh() {
  if (!IS_DESKTOP || autoRefreshTimer) return;

  autoRefreshTimer = window.setInterval(() => {
    if (document.visibilityState === 'visible') {
      refresh();
    }
  }, 2000);
}

function stopAutoRefresh() {
  if (!autoRefreshTimer) return;
  window.clearInterval(autoRefreshTimer);
  autoRefreshTimer = null;
}

document.addEventListener('keydown', async (event) => {
  if (event.key === 'Escape' && IS_DESKTOP) {
    await backend.invoke('close-window');
  }
});

window.addEventListener('beforeunload', stopAutoRefresh);

refresh();
startAutoRefresh();
