function updateNetworkPanel(stats, history, pushHistory) {
  const upMbps = stats.lhmConnected ? stats.net.up : stats.net.up * 8 / 1e6;
  const downMbps = stats.lhmConnected ? stats.net.down : stats.net.down * 8 / 1e6;

  const fmt = (v) => (v >= 1000 ? (v / 1000).toFixed(2) : v.toFixed(1));
  const unit = (v) => (v >= 1000 ? 'Gbps' : 'Mbps');

  pushHistory(history.net, downMbps);

  document.getElementById('netUp').textContent = fmt(upMbps);
  document.getElementById('netUpU').textContent = unit(upMbps);
  document.getElementById('netDown').textContent = fmt(downMbps);
  document.getElementById('netDownU').textContent = unit(downMbps);
  document.getElementById('netIface').textContent = stats.net.iface || '--';
}

export { updateNetworkPanel };
