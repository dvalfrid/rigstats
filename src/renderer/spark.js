function createHistory(length = 80) {
  return {
    cpu: Array(length).fill(0),
    gpu: Array(length).fill(0),
    ram: Array(length).fill(0),
    net: Array(length).fill(0),
    disk: Array(length).fill(0)
  };
}

function pushHistory(series, value) {
  series.push(value);
  series.shift();
}

function drawSpark(id, data, color) {
  const el = document.getElementById(id);
  if (!el) return;

  const width = el.offsetWidth || 402;
  const height = 48;
  el.width = width;
  el.height = height;

  const ctx = el.getContext('2d');
  ctx.clearRect(0, 0, width, height);

  const max = Math.max(...data, 1);
  const points = data.map((v, i) => ({
    x: (i / (data.length - 1)) * width,
    y: height - 0.88 * (v / max) * height - 4
  }));

  const gradient = ctx.createLinearGradient(0, 0, 0, height);
  gradient.addColorStop(0, `${color}44`);
  gradient.addColorStop(1, `${color}00`);

  ctx.beginPath();
  ctx.moveTo(points[0].x, height);
  points.forEach((p) => ctx.lineTo(p.x, p.y));
  ctx.lineTo(points[points.length - 1].x, height);
  ctx.closePath();
  ctx.fillStyle = gradient;
  ctx.fill();

  ctx.beginPath();
  points.forEach((p, i) => {
    if (i === 0) ctx.moveTo(p.x, p.y);
    else ctx.lineTo(p.x, p.y);
  });
  ctx.strokeStyle = color;
  ctx.lineWidth = 1.5;
  ctx.stroke();
}

export { createHistory, pushHistory, drawSpark };
