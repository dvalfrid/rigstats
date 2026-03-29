// Network panel renderer.
// Backend already provides throughput in Mbps, so this module only formats units.

function updateNetworkPanel(stats, history, pushHistory) {
  const upMbps = stats.net.up;
  const downMbps = stats.net.down;

  const fmt = (v) => (v >= 1000 ? (v / 1000).toFixed(2) : v.toFixed(1));
  const unit = (v) => (v >= 1000 ? 'Gbps' : 'Mbps');

  pushHistory(history.netDown, downMbps);
  pushHistory(history.netUp, upMbps);

  document.getElementById('netUp').textContent = fmt(upMbps);
  document.getElementById('netUpU').textContent = unit(upMbps);
  document.getElementById('netDown').textContent = fmt(downMbps);
  document.getElementById('netDownU').textContent = unit(downMbps);
  document.getElementById('netIface').textContent = stats.net.iface || '--';
  document.getElementById('netPing').textContent = Number.isFinite(stats.net.pingMs)
    ? `${Math.round(stats.net.pingMs)} ms`
    : '-- ms';
}

export { updateNetworkPanel };
