const path = require('path');
const fs = require('fs');

const DEFAULTS = { opacity: 0.55, modelName: 'ROG GM700TZ' };

function getSettingsPath(app) {
  return path.join(app.getPath('userData'), 'rigstats-settings.json');
}

function loadSettings(app) {
  try {
    const raw = fs.readFileSync(getSettingsPath(app), 'utf8');
    return { ...DEFAULTS, ...JSON.parse(raw) };
  } catch {
    return { ...DEFAULTS };
  }
}

function saveSettings(app, data) {
  const filePath = getSettingsPath(app);
  fs.mkdirSync(path.dirname(filePath), { recursive: true });
  fs.writeFileSync(filePath, JSON.stringify(data, null, 2), 'utf8');
}

module.exports = { loadSettings, saveSettings };
