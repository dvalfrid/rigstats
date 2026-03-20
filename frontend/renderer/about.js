import { backend, IS_DESKTOP } from './environment.js';

const ids = {
  rigstatsVersion: document.getElementById('rigstatsVersion'),
  rigstatsVersionMeta: document.getElementById('rigstatsVersionMeta'),
  licenseName: document.getElementById('licenseName'),
  websiteLink: document.getElementById('websiteLink'),
  emailLink: document.getElementById('emailLink'),
  refreshBtn: document.getElementById('refreshBtn'),
  closeBtn: document.getElementById('closeBtn'),
  changelogContent: document.getElementById('changelogContent'),
};

function render(info) {
  ids.rigstatsVersion.textContent = info.rigstatsVersion;
  ids.rigstatsVersionMeta.textContent = info.rigstatsVersion;
  ids.licenseName.textContent = info.licenseName;
  ids.emailLink.href = `mailto:${info.contactEmail}`;
  ids.emailLink.textContent = info.contactEmail;
}

async function refresh() {
  if (!IS_DESKTOP) return;

  try {
    render(await backend.invoke('get-about-info'));
  } catch (error) {
    console.error('get-about-info failed:', error);
  }
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
        // Strip markdown links like [text](url) → text, and commit hash links
        const text = item[1].replace(/\[([^\]]+)\]\([^)]+\)/g, '$1').trim();
        if (section === 'feat') features.push(text);
        else fixes.push(text);
      }
    }
    versions.push({ version, url, date, features, fixes });
  }
  return versions;
}

function renderChangelog(md) {
  const versions = parseChangelog(md).slice(0, 6);
  if (!versions.length) return;
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

async function loadChangelog() {
  if (!IS_DESKTOP) return;
  try {
    const md = await backend.invoke('get-changelog');
    renderChangelog(md);
  } catch (error) {
    console.error('get-changelog failed:', error);
  }
}

ids.websiteLink.addEventListener('click', (e) => { e.preventDefault(); backend.openUrl('https://rigstats.app'); });

ids.refreshBtn.addEventListener('click', refresh);
ids.closeBtn.addEventListener('click', async () => {
  if (!IS_DESKTOP) return;
  await backend.invoke('close-window');
});

document.addEventListener('keydown', async (event) => {
  if (event.key === 'Escape' && IS_DESKTOP) {
    await backend.invoke('close-window');
  }
});

refresh();
loadChangelog();
