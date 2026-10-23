/** The ribbon's icons: one family, drawn on a 24 by 24 grid with a 1.5 stroke and `currentColor`,
 *  inline so there is no dependency and no request. Keyed by a button's command (`ribbonLayout.ts`),
 *  so every button of the default ribbon and the quick-access bar has one (`ribbonIcons.test.tsx`
 *  fails when one is missing). The bodies are constants of ours, never user text. */

const dot = (cx: number, cy: number, r = 1.3) => `<circle cx="${cx}" cy="${cy}" r="${r}" fill="currentColor" stroke="none"/>`;
const PENCIL = '<path d="M12.5 21l1-3.5 6.5-6.5 2.5 2.5-6.5 6.5z"/>';
const CUBE = '<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/><path d="M12 12l8-4.5M12 12v9M12 12L4 7.5"/>';
const PRINTER = '<path d="M7 9V3h10v6M7 17H4V9h16v8h-3"/><rect x="7" y="14" width="10" height="7"/>';

/** A constrained dimension: the dimension's glyph with a solid square, the pinned value, on it. */
const pinned = (glyph: string, x: number, y: number) =>
  `${glyph}<rect x="${x - 1.8}" y="${y - 1.8}" width="3.6" height="3.6" fill="currentColor" stroke="none"/>`;

const DIM = {
  linear: '<path d="M4 6v13M20 6v13M4 11h16M4 11l3-1.8M4 11l3 1.8M20 11l-3-1.8M20 11l-3 1.8"/>',
  aligned: '<path d="M3.5 19L8 6.5M15.5 21L20 8.5M5.6 13.5l11-4M5.6 13.5l2.8-.2M5.6 13.5l1.2 2.3M17 9.5l-2.8.2M17 9.5l1.2-2.3"/>',
  angular: '<path d="M4 20h16M4 20L16 5"/><path d="M13 20a9 9 0 00-3.6-7.2"/>',
  radius: '<circle cx="12" cy="12" r="9"/><path d="M12 12l6.5-6.5M14.6 5.3l3.9.2-.2 3.9"/>',
  diameter: '<circle cx="12" cy="12" r="7"/><path d="M4 20L20 4"/>',
};

/** Icon bodies by command. */
export const ICON_BODIES: Record<string, string> = {
  // File and history: the quick-access bar.
  new: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4M12 11v6M9 14h6"/>',
  open: '<path d="M3 7h6l2 2h10v11H3z"/>',
  save: '<path d="M4 4h13l3 3v13H4z"/><path d="M8 4v5h8V4M8 20v-6h8v6"/>',
  saveAs: '<path d="M4 4h11l3 3v4"/><path d="M4 4v16h8M8 4v5h7M15 21l1-3.5 5-5 2.5 2.5-5 5z"/>',
  undo: '<path d="M9 7L4 12l5 5"/><path d="M4 12h10a6 6 0 010 8h-3"/>',
  redo: '<path d="M15 7l5 5-5 5"/><path d="M20 12H10a6 6 0 000 8h3"/>',

  // Measure, inquiry, take-off.
  select: '<path d="M6 3l12 7-5.5 1.6L10 17.5z"/>',
  distance: '<path d="M3 15L15 3l6 6L9 21z"/><path d="M7 11l2.5 2.5M10 8l2.5 2.5M13 5l1.5 1.5"/>',
  area: '<path d="M4 18L7 6l12 3 1 9z"/><path d="M9 16l5-5M12.5 17l5-5"/>',
  count: '<circle cx="6.5" cy="7" r="2.3"/><circle cx="17.5" cy="7" r="2.3"/><circle cx="12" cy="17" r="2.3"/>',
  geometry: '<path d="M3 19L12 4l9 15z"/><path d="M12 4v15" stroke-dasharray="2 2.2"/>',
  divide: '<path d="M3 12h18M6 8.5v7M12 8.5v7M18 8.5v7"/>',
  measureAlong: '<path d="M3 16c5 0 5-9 9-9s4 9 9 9"/><path d="M7 21v-2M12 21v-2M17 21v-2"/>',
  idPoint: '<circle cx="12" cy="12" r="3"/><path d="M12 3v5M12 16v5M3 12h5M16 12h5"/>',
  extract: '<rect x="3" y="4" width="13" height="16" rx="1"/><path d="M3 9h13M8 9v11M14 14h8M19 11l3 3-3 3"/>',
  exportTakeoff: '<rect x="3" y="3" width="14" height="18" rx="1"/><path d="M3 8h14M8 8v13M3 13h10M3 17h10M14 13h8M19 10l3 3-3 3"/>',
  importBoq: '<rect x="7" y="3" width="14" height="18" rx="1"/><path d="M7 8h14M12 8v13M12 13h9M12 17h9M2 13h8M7 10l3 3-3 3"/>',
  compare: '<rect x="3" y="5" width="7.5" height="14"/><rect x="13.5" y="5" width="7.5" height="14"/><path d="M5.5 10h2.5M5.5 14h2.5M16 10h2.5M16 14h2.5"/>',
  exportConflicts: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4M12 11v4.5M12 18.3v.2"/>',

  // Draw.
  line: '<path d="M6 18L18 6"/><rect x="3.5" y="16" width="4.5" height="4.5"/><rect x="16" y="3.5" width="4.5" height="4.5"/>',
  polyline: `<path d="M4 18l5-9 5 6 6-9"/>${dot(4, 18)}${dot(20, 6)}`,
  rectangle: '<rect x="3.5" y="6" width="17" height="12"/>',
  circle: '<circle cx="12" cy="12" r="8.5"/>',
  arc: `<path d="M3.5 18a9.5 9.5 0 0117 0"/>${dot(3.5, 18)}${dot(20.5, 18)}`,
  ellipse: '<ellipse cx="12" cy="12" rx="9.5" ry="5.5"/>',
  spline: '<path d="M3 17C6 3 10 4 12 12s6 8 9-5"/>',
  point: '<path d="M7 7l10 10M17 7L7 17"/>',
  hatch: '<rect x="4" y="4" width="16" height="16"/><path d="M4 13l9-9M4 20L20 4M11 20l9-9"/>',
  revcloud: '<path d="M6.5 18A3.5 3.5 0 015 11.2 4.5 4.5 0 0113 8.5a4 4 0 017 3.2A3.2 3.2 0 0118 18z"/>',
  // Not on the ribbon yet: the revision cloud with a comment (another branch adds its button).
  commentCloud: '<path d="M6.5 14A3.5 3.5 0 015 8.2 4.5 4.5 0 0113 5.5a4 4 0 017 3.2A3.2 3.2 0 0118 14z"/><path d="M4 17.5h10M4 21h6"/>',
  wipeout: '<rect x="4" y="5" width="16" height="14" stroke-dasharray="3 2.2"/><path d="M8 15l8-6"/>',

  // Modify.
  move: '<path d="M12 3v18M3 12h18M9 6l3-3 3 3M9 18l3 3 3-3M6 9l-3 3 3 3M18 9l3 3-3 3"/>',
  copy: '<rect x="8.5" y="8.5" width="12" height="12"/><path d="M15.5 8.5V3.5H3.5v12h5"/>',
  rotate: '<path d="M20 12a8 8 0 11-2.6-5.9"/><path d="M20.5 3.5v5h-5"/>',
  scale: '<rect x="3" y="11" width="10" height="10"/><path d="M13 11l8-8M15 3h6v6"/>',
  mirror: '<path d="M12 3v18" stroke-dasharray="2 2.2"/><path d="M9.5 7L3 19h6.5zM14.5 7L21 19h-6.5z"/>',
  stretch: '<path d="M2.5 12h19M6 8l-3.5 4L6 16M18 8l3.5 4-3.5 4"/><rect x="9" y="8" width="6" height="8"/>',
  align: '<path d="M4 3v18"/><rect x="7" y="5.5" width="13" height="4.5"/><rect x="7" y="14" width="8" height="4.5"/>',
  array: '<rect x="3" y="3" width="7" height="7"/><rect x="14" y="3" width="7" height="7"/><rect x="3" y="14" width="7" height="7"/><rect x="14" y="14" width="7" height="7"/>',
  explode: '<path d="M4 10V4h6M20 10V4h-6M4 14v6h6M20 14v6h-6"/>',
  erase: '<path d="M9 20h11"/><path d="M4.5 14.5l9-9 6 6-6 6H8z"/><path d="M9.5 10l6 6"/>',

  // Edit. Trim: a line cut at the cutting edge, the part beyond it dashed (the mockup's read as a plus).
  trim: '<path d="M3 12h18"/><path d="M6 4l6 8"/><path d="M12 12l6 8" stroke-dasharray="1.8 2.2" opacity=".55"/>',
  extend: '<path d="M19 3v18M3 12h8"/><path d="M12 12h6" stroke-dasharray="2 2.4" opacity=".55"/><path d="M15.5 9l3 3-3 3"/>',
  break: '<path d="M3 12h6M15 12h6M10.5 7l-1.5 10M15 7l-1.5 10"/>',
  join: `<path d="M3 17l9-9 9 9"/>${dot(12, 8, 1.8)}`,
  lengthen: '<path d="M3 5v14M3 12h16M15 8l5 4-5 4"/>',
  reverse: '<path d="M4 8h15l-3.5-3.5M20 16H5l3.5 3.5"/>',
  offset: '<path d="M3 21V9a6 6 0 016-6h12"/><path d="M10 21v-8a3 3 0 013-3h8"/>',
  fillet: '<path d="M4 20V4M4 20h16" stroke-dasharray="1.6 2.6" opacity=".5"/><path d="M4 3v11a6 6 0 006 6h11"/>',
  chamfer: '<path d="M4 20V4M4 20h16" stroke-dasharray="1.6 2.6" opacity=".5"/><path d="M4 3v10l7 7h10"/>',
  blend: '<path d="M3 17h5c5 0 3-10 8-10h5"/>',
  pedit: `<path d="M3 17l4-7 4 5"/>${PENCIL}`,
  overkill: '<rect x="4" y="4" width="12" height="12"/><rect x="8" y="8" width="12" height="12" stroke-dasharray="2 2.2"/>',
  flatten: '<path d="M12 3v7M8.5 7l3.5 3.5L15.5 7M3 15h18M3 20h18"/>',

  // Text.
  text: '<path d="M5 7V4.5h14V7M12 4.5v15M9 19.5h6"/>',
  mtext: '<path d="M4 5h9M8.5 5v10M14 11h7M14 15h7M4 20h17"/>',
  textEdit: `<path d="M3 7V4.5h10V7M8 4.5V15"/>${PENCIL}`,
  find: '<circle cx="10" cy="10" r="6"/><path d="M14.5 14.5L21 21"/>',

  // Clipboard.
  copyclip: '<rect x="5" y="4.5" width="14" height="16.5" rx="1.5"/><path d="M9 4.5v-1.5h6v1.5M8.5 10h7M8.5 14h7"/>',
  copybase: `<rect x="8" y="8" width="12.5" height="12.5"/><path d="M15 8V3.5H3.5V15H8"/>${dot(14.2, 14.2, 1.7)}`,
  pasteclip: '<rect x="5" y="4.5" width="14" height="16.5" rx="1.5"/><path d="M9 4.5v-1.5h6v1.5M12 9.5v7M9 13.5l3 3 3-3"/>',
  pasteblock: '<rect x="5" y="4.5" width="14" height="16.5" rx="1.5"/><path d="M9 4.5v-1.5h6v1.5"/><rect x="9" y="10" width="6" height="6.5"/>',

  // Layers.
  layerStates: '<path d="M12 3l9 5-9 5-9-5z"/><path d="M3 12.5l9 5 9-5M3 17l9 5 9-5"/>',
  translateLayers: '<path d="M3 5h9M7.5 3v2M4.5 10c3-1 5.5-3 6.5-5M5.5 8c1 2.5 3 3.5 5.5 4"/><path d="M12.5 21l4-10 4 10M14 17h5"/>',
  group: '<rect x="2.5" y="2.5" width="19" height="19" stroke-dasharray="2.4 2.2"/><rect x="6" y="6" width="5" height="5"/><rect x="13" y="13" width="5" height="5"/>',
  ungroup: '<rect x="3" y="3" width="8" height="8"/><rect x="13" y="13" width="8" height="8"/>',
  matchprop: '<path d="M4 4h12v5H4z"/><path d="M16 6.5h3.5v6H11v3"/><rect x="9" y="15.5" width="4" height="5.5"/>',
  groupSelect: '<rect x="3" y="3" width="18" height="18" stroke-dasharray="3 2.2"/><path d="M8 12.5l3 3 5-6"/>',

  // Dimensions, leaders, tolerance.
  dimLinear: DIM.linear,
  dimAligned: DIM.aligned,
  dimAngular: DIM.angular,
  dimRadius: DIM.radius,
  dimDiameter: DIM.diameter,
  dimArc: '<path d="M6 19a8 8 0 0112 0M3.5 15a11.5 11.5 0 0117 0M6 19l-2.5-4M18 19l2.5-4"/>',
  dimOrdinate: '<path d="M5 4v16h15"/><path d="M5 12h6l3-5h6"/>',
  dimBaseline: '<path d="M3 3v18M3 7h8M3 12h13M3 17h18M11 5v4M16 10v4M21 15v4"/>',
  dimContinue: '<path d="M3 5v14M10 5v14M18 5v14M3 12h15M5.5 10l-2.5 2 2.5 2M13 10l-3 2 3 2"/>',
  qdim: '<path d="M13 3L5 14h6l-1 7 8-11h-6z"/>',
  dimEdit: `<path d="M3 8v8M17 8v4M3 12h9"/>${PENCIL}`,
  dimBreak: '<path d="M3 7v10M21 7v10M3 12h5M16 12h5M10 8.5l4 7"/>',
  dimSpace: '<path d="M3 4h18M3 12h18M3 20h18M12 4v8M12 12v8"/>',
  dimJogLine: '<path d="M3 12h6l2-5 2 10 2-5h6"/>',
  leader: '<path d="M4 20L14 8h6M4 20l.6-4M4 20l3.6-1.4"/>',
  mleader: '<path d="M3 19l8-10h9M3 7l8 2"/>',
  tolerance: '<rect x="3" y="8" width="18" height="8"/><path d="M9 8v8M15 8v8"/>',
  dimStyle: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',

  // Tables and fields.
  table: '<rect x="3" y="4" width="18" height="16" rx="1"/><path d="M3 9.5h18M3 14.5h18M9 4v16M15 4v16"/>',
  tableEdit: `<path d="M13 20H4V4h16v7"/><path d="M4 9.5h16M4 14.5h7M10 4v16"/>${PENCIL}`,
  field: '<path d="M8 4c-2 0-3 1-3 3v2c0 1.5-1 3-2 3 1 0 2 1.5 2 3v2c0 2 1 3 3 3M16 4c2 0 3 1 3 3v2c0 1.5 1 3 2 3-1 0-2 1.5-2 3v2c0 2-1 3-3 3"/>',
  updateFields: '<path d="M20 12a8 8 0 01-14 5.3M4 12a8 8 0 0114-5.3"/><path d="M18 3v4h-4M6 21v-4h4"/>',

  // Parametric: geometric constraints.
  gcCoincident: `<path d="M3.5 19l8.5-7M12 12l8.5-7"/>${dot(12, 12, 2.2)}`,
  gcCollinear: '<path d="M3 20L10 13M14 9l7-7"/><path d="M10 13l4-4" stroke-dasharray="1.5 2" opacity=".6"/>',
  gcConcentric: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="4.5"/>',
  gcFixed: '<rect x="5" y="11" width="14" height="10" rx="1.5"/><path d="M8 11V8a4 4 0 018 0v3"/>',
  gcParallel: '<path d="M5 20L12 4M12 21l7-16"/>',
  gcPerpendicular: '<path d="M3 20h18M12 20V4"/><path d="M12 15.5h4.5V20"/>',
  gcHorizontal: '<path d="M4 12h16M4 8v8M20 8v8"/>',
  gcVertical: '<path d="M12 4v16M8 4h8M8 20h8"/>',
  gcTangent: '<circle cx="12" cy="9.5" r="6.5"/><path d="M3 16h18"/>',
  gcSmooth: `<path d="M3 17h7c4 0 7-3 7-7V4"/>${dot(10, 17)}`,
  gcSymmetric: '<path d="M12 3v18" stroke-dasharray="2 2.2"/><circle cx="5.5" cy="12" r="1.8"/><circle cx="18.5" cy="12" r="1.8"/><path d="M8 12h8" opacity=".6"/>',
  gcEqual: '<path d="M5 9h14M5 15h14"/>',
  autoConstrain: '<path d="M4 20L15 9"/><path d="M17 3v4M15 5h4M20 11v3M18.5 12.5h3"/>',
  // Dimensional constraints.
  dcLinear: pinned(DIM.linear, 12, 11),
  dcAligned: pinned(DIM.aligned, 11.3, 11.5),
  dcAngular: pinned(DIM.angular, 9.4, 12.8),
  dcRadius: pinned(DIM.radius, 15.2, 8.8),
  dcDiameter: pinned(DIM.diameter, 12, 12),
  parameters: '<path d="M4 20c2.5 0 3-1.5 3.5-4l2-9c.5-2.5 1.2-3 3.5-3"/><path d="M5.5 11.5h8"/><path d="M15 12l6 7M21 12l-6 7"/>',
  delConstraint: '<path d="M5 9h14M5 15h14"/><path d="M4 20L20 4"/>',
  constraintBar: '<rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7.5 12h2M11 12h2M14.5 12h2"/>',

  // Insert: blocks, attributes, libraries.
  block: CUBE,
  blockPalette: '<path d="M10 3l7 4v8l-7 4-7-4V7z"/><path d="M10 11l7-4M10 11v8M10 11L3 7"/><path d="M19 15v6M16 18h6"/>',
  writeBlock: '<path d="M9 3l6 3.5v7L9 17l-6-3.5v-7z"/><path d="M9 10l6-3.5M9 10v7M9 10L3 6.5"/><path d="M16 19h5M19 16.5l2.5 2.5-2.5 2.5"/>',
  refedit: `<path d="M9 3l6 3.5v5M3 6.5V14l6 3.5"/><path d="M9 10l6-3.5M9 10v7.5M9 10L3 6.5"/>${PENCIL}`,
  attdef: `<path d="M3 5h12l6 7-6 7H3z"/>${dot(8, 12, 1.5)}`,
  attributeManager: '<path d="M3 4h9l4 4.5-4 4.5H3z"/><path d="M3 17h18M3 21h12"/>',
  designCenter: '<rect x="3" y="3" width="8" height="8"/><rect x="13" y="3" width="8" height="8"/><rect x="3" y="13" width="8" height="8"/><path d="M13 17h8M17 13v8"/>',
  toolPalettes: `<path d="M12 3a9 9 0 100 18c1.5 0 2-1 1.5-2.2-.5-1.3.3-2.3 1.7-2.3H18a3 3 0 003-3A9 9 0 0012 3z"/>${dot(7.5, 11, 1.2)}${dot(10.5, 7, 1.2)}${dot(15.5, 7.5, 1.2)}`,

  // View.
  zoomExtents: '<path d="M3 9V3h6M21 9V3h-6M3 15v6h6M21 15v6h-6"/><rect x="8" y="8" width="8" height="8"/>',
  zoomWindow: '<rect x="3" y="3" width="11" height="11" stroke-dasharray="2.2 2"/><circle cx="14" cy="14" r="5"/><path d="M17.8 17.8L21.5 21.5"/>',
  views: '<path d="M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  xrefs: '<path d="M10 14a4 4 0 005.7 0l3-3a4 4 0 00-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 00-5.7 0l-3 3a4 4 0 005.7 5.7l1-1"/>',
  attachPdf: '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4"/><path d="M9 17.5v-5h1.8a1.6 1.6 0 010 3.2H9"/>',
  attachImage: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><circle cx="9" cy="10" r="1.8"/><path d="M3 17l5-4.5 4 3.5 3-2.5 6 5"/>',
  calibrate: '<rect x="2.5" y="8" width="19" height="8" rx="1"/><path d="M6.5 8v3M10.5 8v4.5M14.5 8v3M18.5 8v4.5"/>',
  mview: '<rect x="3" y="4" width="18" height="16" rx="1"/><rect x="7" y="8" width="10" height="8"/>',
  split: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M12 4v16"/>',
  ucs: `<path d="M5 19V4M5 19h15M2.5 6.5L5 4l2.5 2.5M17.5 16.5L20 19l-2.5 2.5"/>${dot(5, 19)}`,
  ucsObject: '<path d="M4 20V6M4 20h9"/><circle cx="16.5" cy="8.5" r="4.5"/><path d="M13 12l-4 4"/>',
  ucsWorld: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18"/>',
  sheetSet: '<rect x="7" y="3" width="13" height="15" rx="1"/><path d="M4 7v14h13"/><path d="M10 8h7M10 12h7"/>',
  arabicReversed: '<path d="M12 3v18" stroke-dasharray="2 2.2"/><path d="M8 7H4M8 12H5M8 17H3"/><path d="M16 7h4M16 12h3M16 17h5" opacity=".55"/>',
  ucsIcon: '<path d="M5 19H19M16 16l3 3-3 3M5 19V5M2 8l3-3 3 3"/><rect x="3.5" y="17.5" width="3" height="3"/>',

  // Manage.
  runScript: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M10 9l5 3-5 3z"/>',
  audit: '<path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"/><path d="M8.5 12l2.5 2.5 4.5-5"/>',
  purge: '<path d="M4 7h16M9 7V4h6v3M6 7l1 14h10l1-14"/><path d="M10 11v6M14 11v6"/>',
  recover: '<path d="M3.5 12a8.5 8.5 0 102.6-6.1"/><path d="M3 4v5h5"/><path d="M12 8v4l3 2"/>',
  options: '<circle cx="12" cy="12" r="3"/><path d="M12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1"/>',
  customize: '<rect x="3" y="4" width="18" height="16" rx="1.5"/><path d="M3 9h18M7 6.5h.01M10 6.5h.01M8 13.5h8M8 16.5h5"/>',
  aliases: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 10l3 2.5L7 15M12.5 15H17"/>',

  // Output.
  plot: PRINTER,
  quickPrint: '<path d="M6 8V3h12v5M6 16H3V8h18v8h-3"/><path d="M13.5 11l-3.5 5.5h3.5L12.5 21l4-6h-3.5z"/>',
  printAll: '<path d="M6 9V3h12v6"/><path d="M6 17H3V9h18v8h-3"/><path d="M7 14h10v3.5H7z"/><path d="M9 20.5h6"/>',
  plotStyles: '<path d="M3 6h11" stroke-width="1"/><path d="M3 12h11" stroke-width="2.2"/><path d="M3 18h11" stroke-width="3.6"/><circle cx="19" cy="6" r="1.6"/><circle cx="19" cy="12" r="1.6"/><circle cx="19" cy="18" r="1.6"/>',

  // The chrome around the buttons: group overflow, the preferences cluster.
  more: `<circle cx="5" cy="12" r="1.5" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.5" fill="currentColor" stroke="none"/><circle cx="19" cy="12" r="1.5" fill="currentColor" stroke="none"/>`,
  chevron: '<path d="M7 10l5 5 5-5"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6L7 7M17 17l1.4 1.4M5.6 18.4L7 17M17 7l1.4-1.4"/>',
  moon: '<path d="M20 14.5A8.5 8.5 0 019.5 4 8.5 8.5 0 1020 14.5z"/>',
  gear: '<path d="M10.3 3.5h3.4l.5 2.4 1.7.9 2.3-.8 1.7 2.9-1.8 1.6v2l1.8 1.6-1.7 2.9-2.3-.8-1.7.9-.5 2.4h-3.4l-.5-2.4-1.7-.9-2.3.8-1.7-2.9L5.3 13v-2L3.5 9.4l1.7-2.9 2.3.8 1.7-.9z"/><circle cx="12" cy="12" r="2.6"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.8v.2"/>',
};

/** Icons that point along the text: they mirror in a right-to-left window. */
export const MIRRORED = new Set(["undo", "redo", "extend", "lengthen", "runScript"]);

export const hasIcon = (command: string) => command in ICON_BODIES;

/** One icon; `size` is left to CSS (the ribbon's size tokens), which sets the svg's width and height. */
export function RibbonIcon({ name, className }: { name: string; className?: string }) {
  const classes = ["ic", MIRRORED.has(name) ? "ic-mirrored" : "", className ?? ""].filter(Boolean).join(" ");
  return (
    <svg
      className={classes}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      dangerouslySetInnerHTML={{ __html: ICON_BODIES[name] ?? "" }}
    />
  );
}
