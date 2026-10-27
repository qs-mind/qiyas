(() => {
  const listeners = {}; let n = 0;
  const lang = new URLSearchParams(location.search).get('lang') || localStorage.getItem('qiyas.lang') || 'en';
  try { localStorage.setItem('qiyas.lang', lang); } catch {}
  const q = new URLSearchParams(location.search);
  const state = q.get('state') || 'chat';
  const PLAN = '/d/Cairo villa.dwg';
  const T = (x) => x;
  const info = (path) => ({ path, untitled: false, sheets: [
      { name: 'Model', block: '*Model_Space', is_model: true },
      { name: 'A-101', block: '*Paper_Space', is_model: false }, { name: 'A-102', block: '*Paper_Space0', is_model: false },
      { name: 'A-201', block: '*Paper_Space1', is_model: false }, { name: 'A-301', block: '*Paper_Space2', is_model: false },
      { name: 'A-501', block: '*Paper_Space3', is_model: false }, { name: 'A-602', block: '*Paper_Space4', is_model: false },
      { name: 'A-701-LONG-LAYOUT-NAME', block: '*Paper_Space5', is_model: false }, { name: 'A-702-ANOTHER-LONG-NAME', block: '*Paper_Space6', is_model: false },
      { name: 'S-101', block: '*Paper_Space7', is_model: false }, { name: 'S-102', block: '*Paper_Space8', is_model: false } ],
    layers: ['0','A-GF-WALL-EXTR-FINISH','A-GF-DOOR','A-GF-GLAZ-WINDOW-FRAME','A-FF-WALL-INT-PARTITION','A-ANNO-TTLB','XREF$0$S-STRUCT-COLUMN-GRID-LONGNAME'].map((name, i) => ({ name, color: [200, 80 + i * 20, 90], off: false, frozen: false, locked: false })),
    units: 'metres', problems: [], arabic_reversed: false, xrefs: [], sheet: '*Model_Space', hidden_layers: [],
    current: { layer: '0', color: 'ByLayer', linetype: 'ByLayer', lineweight: -1 }, text_styles: ['Standard'], text_style: 'Standard',
    dim_styles: [], dim_style: 'Standard', annotation_scale: '1:100', annotation_factor: 100, block_edit: null });
  const card = (id, label, effect, cat, mutating = false) => [
    { type: 'event', event: { type: 'tool_call', id, name: mutating ? 'qiyas_execute' : 'qiyas_read', label, input: {}, mutating, activity: { category: cat, effect } } },
    { type: 'event', event: { type: 'tool_result', id, summary: 'ok', count: null, handles: [], ui: null, is_error: false } } ];
  const transcript = [
    { type: 'user', text: lang === 'ar' ? 'احصر الدور الأرضي وأنشئ جدول الكميات.' : 'Take off the ground floor and build the BOQ.', pointed: [], images: [] },
    ...['a','b','c','d','e','f','g','h'].flatMap((k) => card(k, 'records', 'read', 'drawing')),
    ...card('m1', 'draw', 'edit', 'drawing', true),
    ...['i','j','k'].flatMap((k) => card(k, 'check', 'read', 'drawing')),
    { type: 'event', event: { type: 'text', text: lang === 'ar'
        ? 'تم قياس الدور الأرضي: ٨ غرف و١٧ بابًا ونافذة. كل رقم مرتبط بالرسم. أحتاج إجابة واحدة قبل أن يصبح جدول الكميات نهائيًا (النظام: POMI).'
        : 'The ground floor is measured: 8 rooms and 17 doors and windows. Every number is traced to the drawing. I need one answer before the BOQ is final (method: POMI).' } },
    { type: 'event', event: { type: 'question', questions: [ { id: 'q1', allow_other: true,
        question: lang === 'ar' ? 'أي دولة تحكم أكواد هذه الفيلا؟' : 'Which country’s codes govern this villa?',
        options: lang === 'ar'
          ? [ { label: 'مصر', description: 'يتوافق مع النص العربي على الرسم', recommended: true }, { label: 'السعودية' }, { label: 'الإمارات' } ]
          : [ { label: 'Egypt', description: 'Matches the Arabic text on the drawing', recommended: true }, { label: 'Saudi Arabia' }, { label: 'UAE' } ] } ] } },
  ];
  const data = {
    aliases: () => ({}), recents: () => [], takeoff: () => [{ id: 'm1', kind: 'area', value: 28, unit: 'm2', shown: 28, description: 'Living', layer: 'A-ROOM', sheet: '*Model_Space', handles: [1], points: [], block_name: null, self_intersecting: false, source: 'ai', note: '', created: '', boq_code: null, chain: [], block_path: [] }],
    totals: () => [], conflicts: () => [], paper_views: () => [], measurement_methods: () => [], ai_conversations: () => [],
    measurement_report: () => ({ items: [], warnings: [] }),
    boq: () => ({ source: '/d/bill.xlsx', imported: '', skipped_rows: 0, items: [
      { code: '1.1', description: 'Brick wall 25 cm, external', unit: 'm2', quantity: 124.6, rate: null, section: 'Masonry', tolerance: null, row: 2 },
      { code: '1.2', description: 'Brick wall 12 cm, internal', unit: 'm2', quantity: 112.32, rate: null, section: 'Masonry', tolerance: null, row: 3 },
      { code: '2.1', description: 'Porcelain floor tile', unit: 'm2', quantity: 114.73, rate: null, section: 'Floors', tolerance: null, row: 4 } ] }),
    get_settings: () => ({ provider: 'anthropic', providers: { anthropic: { model: 'claude-opus-5-5' }, openai_compatible: { model: '', base_url: null } }, autosave_minutes: 5, mcp_enabled: false, mcp_port: 48123, mcp_token: 'test-token', language: lang, theme: 'light', aliases: {}, measurement_method: 'POMI' }),
    app_dir: () => '/opt/Qiyas', mcp_status: () => ({ enabled: false, listening: false, addr: null, error: null }),
    get_default_ribbon: null, recoverable_drawings: () => [],
    drawing_info: () => info(PLAN), tabs: () => ({ tabs: [ { id: 1, path: PLAN, title: 'Cairo villa.dwg', dirty: true, busy: true, status: 'working' }, { id: 2, path: '/d/Second.dwg', title: 'Second.dwg', dirty: false, status: 'idle' } ], active: 1 }),
    get_drafting: () => ({ osnap: true, snap_modes: {}, ortho: false, polar: false, polar_step_deg: 45, grid: false, grid_spacing: 10, grid_snap: false, precision: 3 }),
    batch_version: () => 1, render_batch: () => new Uint8Array([0x51, 0x49, 0x4e, 0x4d, 0, 0, 0, 0]).buffer,
    workspace_attach: (a) => ({ workspace: 1, cursor: 0, transcript: state === 'empty' ? [] : transcript, status: '', error: null, choice: { provider: 'anthropic', model: 'claude-opus-5-5', effort: null }, context: {} }),
    ai_status: () => ({ provider: 'anthropic', model: 'claude-opus-5-5', has_key: true, busy: false, message_count: 0 }),
  };
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
    transformCallback: (cb) => { const id = ++n; window['_' + id] = cb; return id; },
    unregisterCallback: () => {}, convertFileSrc: (p) => p,
    invoke: async (cmd, args) => {
      if (cmd === 'plugin:event|listen') { (listeners[args.event] ??= []).push(args.handler); return args.handler; }
      if (cmd.startsWith('plugin:')) return null;
      const h = data[cmd];
      if (h === undefined) return null;
      if (h === null) return { tabs: [{ id: 'home', label: 'home', groups: [] }] };
      return typeof h === 'function' ? h(args) : h;
    },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
})();
