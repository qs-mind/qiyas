/** The small icons of the new shell (W4): the same family as the ribbon's (24 grid, 1.5 stroke,
 *  `currentColor`), drawn inline. Arrows that point along the text carry `ic-mirrored`, so Arabic
 *  turns them round. */
const BODIES: Record<string, string> = {
  /** Points to where the text goes on: right in English, left in Arabic. */
  next: '<path d="M9 6l6 6-6 6"/>',
  down: '<path d="M6 9l6 6 6-6"/>',
  close: '<path d="M6 6l12 12M18 6L6 18"/>',
  /** A chip: the model Mizan runs on. */
  model: '<rect x="6.5" y="6.5" width="11" height="11" rx="2"/><path d="M10 3v3.5M14 3v3.5M10 17.5V21M14 17.5V21M3 10h3.5M3 14h3.5M17.5 10H21M17.5 14H21"/>',
  /** An AI app plugging in. */
  plug: '<path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 01-12 0zM12 17v4"/>',
  /** Outputs: stacked sheets. */
  outputs: '<path d="M12 3.5l9 5-9 5-9-5z"/><path d="M3 13l9 5 9-5"/>',
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
