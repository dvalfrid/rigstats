import { backend, IS_DESKTOP } from './environment.js';

const ids = {
  rigstatsVersion: document.getElementById('rigstatsVersion'),
  taskName: document.getElementById('taskName'),
  taskHealth: document.getElementById('taskHealth'),
  taskToRun: document.getElementById('taskToRun'),
  logPath: document.getElementById('logPath'),
  lastSuccessAt: document.getElementById('lastSuccessAt'),
  logTail: document.getElementById('logTail'),
  dependenciesTable: document.getElementById('dependenciesTable'),
  copyState: document.getElementById('copyState'),
  refreshBtn: document.getElementById('refreshBtn'),
  copyLogBtn: document.getElementById('copyLogBtn'),
  closeBtn: document.getElementById('closeBtn'),
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

function setTaskText(element, value) {
  element.textContent = value && String(value).trim() ? value : '--';
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

function setTaskHealth(info) {
  const rawStatus = (info.lhmTaskStatus || '').trim().toLowerCase();
  const rawResult = (info.lhmTaskLastResult || '').trim().toLowerCase();

  let label = 'Unknown';
  let className = 'health-neutral';

  if (!info.lhmTaskName) {
    label = 'Missing';
    className = 'health-bad';
  } else if (info.lhmConnected) {
    label = 'Success';
    className = 'health-good';
  } else if (rawStatus.includes('running') || rawResult === '267009' || rawResult === '0x41301') {
    label = 'Running';
    className = 'health-warn';
  } else if (rawResult === '0' || rawResult === '0x0') {
    label = 'Success';
    className = 'health-good';
  } else if (rawStatus.includes('disabled') || rawStatus.includes('failed')) {
    label = 'Failed';
    className = 'health-bad';
  } else if (rawResult) {
    label = 'Failed';
    className = 'health-bad';
  }

  ids.taskHealth.textContent = label;
  ids.taskHealth.className = `meta-value ${className}`;
  ids.taskHealth.title = rawResult
    ? `Derived from Windows Task Scheduler state/result. Raw result: ${info.lhmTaskLastResult}`
    : 'Derived from Windows Task Scheduler state/result.';
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
  setTaskText(ids.taskName, info.lhmTaskName);
  setTaskHealth(info);
  setTaskText(ids.taskToRun, info.lhmTaskToRun);
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
