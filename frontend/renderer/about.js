import { backend, IS_DESKTOP } from './environment.js';

const ids = {
  rigstatsVersion: document.getElementById('rigstatsVersion'),
  rigstatsVersionMeta: document.getElementById('rigstatsVersionMeta'),
  licenseName: document.getElementById('licenseName'),
  githubLink: document.getElementById('githubLink'),
  emailLink: document.getElementById('emailLink'),
  refreshBtn: document.getElementById('refreshBtn'),
  closeBtn: document.getElementById('closeBtn'),
};

function render(info) {
  ids.rigstatsVersion.textContent = info.rigstatsVersion;
  ids.rigstatsVersionMeta.textContent = info.rigstatsVersion;
  ids.licenseName.textContent = info.licenseName;
  ids.githubLink.href = info.githubUrl;
  ids.githubLink.textContent = info.githubUrl;
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
