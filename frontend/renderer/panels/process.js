// Process monitor panel renderer.
// Displays the top processes sorted by CPU usage.

const MAX_NAME_LEN = 16;

function escapeHtml(str) {
  return str.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// Exported for unit tests.
function formatRam(mb) {
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)}G`;
  return `${mb}M`;
}

// Strip common Windows executable suffix and truncate to MAX_NAME_LEN.
// Exported for unit tests.
function truncateName(name) {
  const stripped = name.replace(/\.exe$/i, '');
  if (stripped.length <= MAX_NAME_LEN) return stripped;
  return stripped.substring(0, MAX_NAME_LEN);
}

function updateProcessPanel(processes) {
  const list = document.getElementById('procList');
  if (!list) return;

  if (!processes || processes.length === 0) {
    list.innerHTML = '<div class="proc-row"><span class="proc-name null-val">--</span></div>';
    return;
  }

  list.innerHTML = processes.map((p) => `
    <div class="proc-row">
      <span class="proc-name">${escapeHtml(truncateName(p.name))}</span>
      <span class="proc-cpu">${Math.max(0, p.cpu).toFixed(1)}%</span>
      <span class="proc-ram">${formatRam(p.memMb)}</span>
    </div>`).join('');
}

export { updateProcessPanel, truncateName, formatRam, escapeHtml };
