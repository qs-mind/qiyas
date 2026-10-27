// Runs after scripts/perf/stub.js: bigger data through the same faked IPC, from the page's query.
(() => {
  const q = new URLSearchParams(location.search);
  const layers = +(q.get('layers') || 0), items = +(q.get('items') || 0), msgs = +(q.get('msgs') || 0);
  const inv = window.__TAURI_INTERNALS__.invoke;
  window.__TAURI_INTERNALS__.invoke = async (cmd, args) => {
    const r = await inv(cmd, args);
    if (cmd === 'drawing_info' && layers && r) {
      r.layers = Array.from({ length: layers }, (_, i) => ({ name: `A-${String(i % 40).padStart(2, '0')}-LAYER-${i}`, color: [200, (i * 7) % 255, 90], off: false, frozen: false, locked: false }));
    }
    if (cmd === 'takeoff' && items) {
      const base = r[0];
      return Array.from({ length: items }, (_, i) => ({ ...base, id: 'm' + i, value: 1 + i * 0.37, shown: 1 + i * 0.37, description: 'Room ' + i, layer: 'A-ROOM-' + (i % 30), handles: [i + 1] }));
    }
    if (cmd === 'workspace_attach' && msgs && r && r.transcript) {
      const t = r.transcript, out = [];
      for (let i = 0; i < msgs; i++) out.push(...t.map((m, k) => (m.type === 'event' && m.event.id ? { ...m, event: { ...m.event, id: m.event.id + '_' + i } } : m)));
      return { ...r, transcript: out };
    }
    return r;
  };
})();
