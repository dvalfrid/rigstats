import { backend, IS_DESKTOP } from './environment.js';

const ids = {
  heroTitle: document.getElementById('heroTitle'),
  currentVersion: document.getElementById('currentVersion'),
  newVersion: document.getElementById('newVersion'),
  changelogContent: document.getElementById('changelogContent'),
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

function parseChangelog(md) {
  const versions = [];
  const blocks = md.split(/\n(?=## \[)/);
  for (const block of blocks) {
    const header = /^## \[([^\]]+)\](?:\(([^)]+)\))?\s*(?:\(([^)]+)\))?/.exec(block);
    if (!header) continue;
    const version = header[1];
    if (version.toLowerCase() === 'unreleased') continue;
    const url = header[2] ?? null;
    const date = header[3] ?? '';
    const features = [];
    const fixes = [];
    let section = null;
    for (const line of block.split('\n').slice(1)) {
      if (/^### Features/.test(line)) { section = 'feat'; continue; }
      if (/^### Bug Fixes/.test(line)) { section = 'fix'; continue; }
      if (/^###/.test(line)) { section = null; continue; }
      const item = /^\*\s+(.+)/.exec(line);
      if (item && section) {
        const text = item[1].replace(/\[([^\]]+)\]\([^)]+\)/g, '$1').trim();
        if (section === 'feat') features.push(text);
        else fixes.push(text);
      }
    }
    versions.push({ version, url, date, features, fixes });
  }
  return versions;
}

function renderNotes(body) {
  ids.changelogContent.textContent = '';

  if (!body) {
    const p = document.createElement('div');
    p.className = 'cl-empty';
    p.textContent = 'No release notes available.';
    ids.changelogContent.appendChild(p);
    return;
  }

  const versions = parseChangelog(body);

  if (!versions.length) {
    // Body is plain text, not structured markdown — display as-is.
    const p = document.createElement('div');
    p.className = 'cl-empty';
    p.textContent = body;
    ids.changelogContent.appendChild(p);
    return;
  }

  const frag = document.createDocumentFragment();
  for (const v of versions) {
    const wrap = document.createElement('div');
    wrap.className = 'cl-version';

    const hdr = document.createElement('div');
    hdr.className = 'cl-version-header';
    let tag;
    if (v.url) {
      tag = document.createElement('a');
      tag.href = '#';
      tag.addEventListener('click', (e) => { e.preventDefault(); backend.openUrl(v.url); });
    } else {
      tag = document.createElement('span');
    }
    tag.className = 'cl-version-tag';
    tag.textContent = `v${v.version}`;
    const date = document.createElement('span');
    date.className = 'cl-version-date';
    date.textContent = v.date;
    hdr.append(tag, date);
    wrap.appendChild(hdr);

    for (const [label, items, cls] of [['Features', v.features, 'feat'], ['Bug Fixes', v.fixes, 'fix']]) {
      if (!items.length) continue;
      const lbl = document.createElement('div');
      lbl.className = 'cl-section-label';
      lbl.textContent = label;
      const ul = document.createElement('ul');
      ul.className = 'cl-items';
      for (const text of items) {
        const li = document.createElement('li');
        li.className = cls;
        li.textContent = text;
        ul.appendChild(li);
      }
      wrap.append(lbl, ul);
    }
    frag.appendChild(wrap);
  }
  ids.changelogContent.appendChild(frag);
}

async function loadUpdateInfo() {
  if (!IS_DESKTOP) {
    ids.heroTitle.textContent = 'Browser mode';
    ids.updateBtn.disabled = true;
    setStatus('Update checks are only available in the desktop app.');
    return;
  }

  try {
    const info = await backend.invoke('check-for-update');
    if (!info) {
      // No update — show bundled changelog so the window is still useful.
      ids.heroTitle.textContent = 'Up to Date';
      ids.updateBtn.disabled = true;
      setStatus('You are running the latest version.');
      const md = await backend.invoke('get-changelog').catch(() => '');
      renderNotes(md);
      return;
    }
    ids.currentVersion.textContent = `v${info.currentVersion}`;
    ids.newVersion.textContent = `v${info.version}`;
    // info.body comes from latest.json and contains the new version's changelog.
    renderNotes(info.body);
  } catch (err) {
    ids.updateBtn.disabled = true;
    setStatus(`Error: ${err}`);
    renderNotes(null);
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
