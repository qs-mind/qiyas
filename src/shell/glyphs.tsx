/** The small icons of the new shell (W4): the same family as the ribbon's (24 grid, 1.5 stroke,
 *  `currentColor`), drawn inline. Arrows that point along the text carry `ic-mirrored`, so Arabic
 *  turns them round. */
const BODIES: Record<string, string> = {
  /** Points to where the text goes on: right in English, left in Arabic. */
  next: '<path d="M9 6l6 6-6 6"/>',
  down: '<path d="M6 9l6 6 6-6"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
  /** An AI app plugging in. */
  plug: '<path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 01-12 0zM12 17v4"/>',
  /** Artifacts: a sheet with its corner folded and a spark on it. */
  artifacts: '<path d="M14 3.5H7.5a2 2 0 00-2 2v13a2 2 0 002 2h9a2 2 0 002-2V8z"/><path d="M14 3.5V8h4.5"/><path d="M12 11.2c.35 1.75 1.5 2.9 3.25 3.25-1.75.35-2.9 1.5-3.25 3.25-.35-1.75-1.5-2.9-3.25-3.25 1.75-.35 2.9-1.5 3.25-3.25z"/>',
  /** Attach a picture or a PDF: a paperclip. */
  attach: '<path d="M20 11.6l-7.9 7.9a5 5 0 01-7.1-7.1l8.4-8.4a3.3 3.3 0 014.7 4.7l-8.3 8.3a1.65 1.65 0 01-2.35-2.35L15 7.1"/>',
  /** Point at part of the drawing: a dashed box and a pointer. */
  area: '<path d="M15 3.5H5A1.5 1.5 0 003.5 5v10" stroke-dasharray="2.4 2.6"/><path d="M10.5 10.5l10 3.8-4.3 1.8-1.8 4.4z"/>',
  /** The model Mizan thinks with: a spark. */
  spark: '<path d="M10.5 4c.6 3.6 2.4 5.4 6 6-3.6.6-5.4 2.4-6 6-.6-3.6-2.4-5.4-6-6 3.6-.6 5.4-2.4 6-6z"/><path d="M18 14.5c.25 1.4.85 2 2.25 2.25-1.4.25-2 .85-2.25 2.25-.25-1.4-.85-2-2.25-2.25 1.4-.25 2-.85 2.25-2.25z"/>',
  keyboard: '<rect x="3" y="6" width="18" height="12" rx="2"/><path d="M7 10h.01M11 10h.01M15 10h.01M7 14h10"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V5a1 1 0 00-1-1H5a1 1 0 00-1 1v10a1 1 0 001 1h3"/>',
  /** A slash command. */
  slash: '<path d="M16 4L8 20"/>',
  newChat: '<path d="M12 5v14M5 12h14"/>',
  history: '<path d="M4 12a8 8 0 108-8 8 8 0 00-6 2.7L4 9"/><path d="M4 4v5h5M12 8v4l3 2"/>',
  arrowUp: '<path d="M12 19V5M6 11l6-6 6 6"/>',
  stop: '<rect x="7" y="7" width="10" height="10" rx="1.5"/>',
};

export function Glyph({ name, mirror = false, size = 16 }: { name: keyof typeof BODIES | string; mirror?: boolean; size?: number }) {
  return (
    <svg
      className={mirror ? "gl ic-mirrored" : "gl"}
      viewBox="0 0 24 24"
      width={size}
      height={size}
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      dangerouslySetInnerHTML={{ __html: BODIES[name] ?? "" }}
    />
  );
}
