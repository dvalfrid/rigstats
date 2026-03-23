import { backend, IS_DESKTOP } from './environment.js';

const ids = {
  rigstatsVersion: document.getElementById('rigstatsVersion'),
  rigstatsVersionMeta: document.getElementById('rigstatsVersionMeta'),
  licenseName: document.getElementById('licenseName'),
  websiteLink: document.getElementById('websiteLink'),
  emailLink: document.getElementById('emailLink'),
  refreshBtn: document.getElementById('refreshBtn'),
  closeBtn: document.getElementById('closeBtn'),
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
