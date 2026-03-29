// Sparkline utilities.
// Maintains fixed-length metric history arrays and draws lightweight canvas charts.

function createHistory(length = 80) {
  return {
    cpu: Array(length).fill(0),
    gpu: Array(length).fill(0),
    ram: Array(length).fill(0),
    netDown: Array(length).fill(0),
    netUp: Array(length).fill(0),
    disk: Array(length).fill(0),
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
  const height = el.offsetHeight || 48;
  el.width = width;
  el.height = height;

  const ctx = el.getContext('2d');
  ctx.clearRect(0, 0, width, height);

  const max = Math.max(...data, 1);
  const points = data.map((v, i) => ({
    x: (i / (data.length - 1)) * width,
    y: height - 0.88 * (v / max) * height - 4,
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

// Draws two overlaid series on the same canvas, normalised against a shared max.
// data1 (e.g. download) is drawn with a filled area; data2 (e.g. upload) is a
// line only so it doesn't obscure the primary series.
function drawDoubleSpark(id, data1, color1, data2, color2) {
  const el = document.getElementById(id);
  if (!el) return;

  const width = el.offsetWidth || 402;
  const height = el.offsetHeight || 48;
  el.width = width;
  el.height = height;

  const ctx = el.getContext('2d');
  ctx.clearRect(0, 0, width, height);

  const max = Math.max(...data1, ...data2, 1);

  function toPoints(data) {
    return data.map((v, i) => ({
      x: (i / (data.length - 1)) * width,
      y: height - 0.88 * (v / max) * height - 4,
    }));
  }

  // Draw filled area + line for data1.
  const pts1 = toPoints(data1);
  const grad = ctx.createLinearGradient(0, 0, 0, height);
  grad.addColorStop(0, `${color1}44`);
  grad.addColorStop(1, `${color1}00`);
  ctx.beginPath();
  ctx.moveTo(pts1[0].x, height);
  pts1.forEach((p) => ctx.lineTo(p.x, p.y));
  ctx.lineTo(pts1[pts1.length - 1].x, height);
  ctx.closePath();
  ctx.fillStyle = grad;
  ctx.fill();

  ctx.beginPath();
  pts1.forEach((p, i) => { if (i === 0) ctx.moveTo(p.x, p.y); else ctx.lineTo(p.x, p.y); });
  ctx.strokeStyle = color1;
  ctx.lineWidth = 1.5;
  ctx.stroke();

  // Draw line only for data2 (upload sits on top without obscuring the fill).
  const pts2 = toPoints(data2);
  ctx.beginPath();
  pts2.forEach((p, i) => { if (i === 0) ctx.moveTo(p.x, p.y); else ctx.lineTo(p.x, p.y); });
  ctx.strokeStyle = color2;
  ctx.lineWidth = 1.5;
  ctx.stroke();
}

export { createHistory, pushHistory, drawSpark, drawDoubleSpark };
