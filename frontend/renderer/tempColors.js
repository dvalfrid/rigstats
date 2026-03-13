const TEMP_OK_COLOR = '#39ff88';
const TEMP_WARM_COLOR = '#ffb300';
const TEMP_HOT_COLOR = '#ff3a1f';
const TEMP_UNKNOWN_COLOR = '#6f8db7';

function resolveTempColor(value, warmAt, hotAt) {
  if (!Number.isFinite(value) || value <= 0) return TEMP_UNKNOWN_COLOR;
  if (value >= hotAt) return TEMP_HOT_COLOR;
  if (value >= warmAt) return TEMP_WARM_COLOR;
  return TEMP_OK_COLOR;
}

export {
  TEMP_HOT_COLOR,
  TEMP_OK_COLOR,
  TEMP_UNKNOWN_COLOR,
  TEMP_WARM_COLOR,
  resolveTempColor
};