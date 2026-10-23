import { entityKind } from "../i18n/entityKind";
import { memo, useEffect, useId, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import type { Current, EntityInfo, EntityProp, Units } from "../api";
import { useT } from "../i18n";
import { normalizeDigits } from "../tools/commandParser";
import { metresPerUnit } from "./AttachPdfDialog";
import { LINEWEIGHTS, weightLabel } from "./lineweights";
import { geometryRows, selectionRows, summarise, type GeoRow } from "./propertiesGeometry";

type Props = {
  /** The entity picked with the select tool; null when none is. */
  entity: EntityInfo | null;
  units: Units;
  /** Decimals of lengths and areas. */
  precision: number;
  /** Two or more entities picked together (selection set or a box): the palette shows how many,
   *  the sums of their length and area and the General fields they share, read-only. */
  many?: EntityInfo[] | null;
  /** The name of the sheet in view. */
  sheet: string;
  /** The drawing's own layers and linetypes, for the editable fields. */
  layers?: string[];
  linetypes?: string[];
  /** Edits the picked entity: only a drawing's own entity at the top of the sheet (never an
   *  xref's or one inside a block). Without it the palette is read-only. Lengths go in drawing
   *  units, angles in degrees. */
  onSet?(prop: EntityProp, value: boolean | string | number | [number, number]): void;
  /** With nothing picked: what new entities are drawn with, and changing it. */
  current?: Current;
  onSetCurrent?(current: Current): void;
  /** Counts up when something asks to see the palette (the canvas menu's Properties): it opens,
   *  if it was folded, and takes the focus. */
  reveal?: number;
};

/** A number typed in the window's digits (Arabic ones too); null when it is none. */
function typedNumber(text: string): number | null {
  const v = Number(normalizeDigits(text.trim()).replace("٫", "."));
  return text.trim() !== "" && Number.isFinite(v) ? v : null;
}

/** Runs `commit` on Enter. */
const onEnter = (commit: () => void) => (e: KeyboardEvent<HTMLInputElement>) => {
  if (e.key === "Enter") {
    e.preventDefault();
    commit();
  }
};

/** A text field that sends its value on Enter or when it loses focus, if it changed. */
function Field({ label, value, onCommit, dir = "ltr" }: { label: string; value: string; onCommit(v: string): void; dir?: string }) {
  const [text, setText] = useState(value);
  const commit = () => text !== value && onCommit(text);
  return (
    <input
      aria-label={label}
      dir={dir}
      value={text}
      onChange={(e) => setText(e.target.value)}
      onKeyDown={onEnter(commit)}
      onBlur={commit}
    />
  );
}

/** "ByLayer" and "ByBlock" first, then `names` (and `value` when it is none of them). */
function Choice({ label, value, names, onPick }: { label: string; value: string; names: string[]; onPick(v: string): void }) {
  const t = useT();
  const all = names.includes(value) ? names : [value, ...names];
  return (
    <select aria-label={label} value={value} onChange={(e) => onPick(e.target.value)}>
      {all.map((n) => (
        <option key={n} value={n}>
          {n === "ByLayer" ? t("props.byLayer") : n === "ByBlock" ? t("props.byBlock") : n}
        </option>
      ))}
    </select>
  );
}

const FOLDED = "qiyas.properties.folded";

/** The sections the user folded, remembered on this computer. */
function storedFolded(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(FOLDED) ?? "[]");
    return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** A read-only value: selectable text that a click copies (the number, without its unit). */
function Value({ row }: { row: GeoRow }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy() {
    try {
      await navigator.clipboard.writeText(row.copy);
    } catch {
      return; // The text stays selectable.
    }
    setCopied(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setCopied(false), 1200);
  }
  return (
    <button
      type="button"
      className="value"
      data-copied={copied || undefined}
      title={t(copied ? "props.copied" : "props.copy")}
      onClick={copy}
    >
      {row.text}
    </button>
  );
}

/** A fold-able block of rows, like AutoCAD's Properties sections. */
function Section({
  id,
  title,
  folded,
  onFold,
  className = "",
  children,
}: {
  id: string;
  title: string;
  folded: boolean;
  onFold(): void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div className={`prop-section ${className}`.trim()}>
      <h3>
        <button type="button" aria-expanded={!folded} aria-controls={id} onClick={onFold}>
          {title}
        </button>
      </h3>
      {!folded && <div id={id}>{children}</div>}
    </div>
  );
}

type Row = [label: string, value: ReactNode, text?: boolean];
type SectionData = { id: string; title: string; rows: Row[]; className?: string };

/** The picked entity's properties in AutoCAD's sections: General (handle, type, layer, colour,
 *  linetype, lineweight, sheet, where it sits) and Geometry (`propertiesGeometry`: ends, centre,
 *  radius, angles, length, area, vertices, insertion, pattern, text... in metres, square metres and
 *  degrees with the drafting precision, a click copies a value), then Attributes. Several entities
 *  picked together (`many`) show Selection (how many, of which kinds, the sums of length and area)
 *  and the General fields they share. Sections fold, remembered. A drawing's own entity at the top of the
 *  sheet is edited in place (`onSet`): its layer, colour, linetype, lineweight, and as it has them
 *  its text, text height and rotation, radius, closed flag, global width, scale and rotation.
 *  With nothing picked it edits what new entities are drawn with (`onSetCurrent`).
 *  Collapsible; memoised, as the app re-renders on every pointer move. */
export const PropertiesPanel = memo(function PropertiesPanel({
  entity,
  many = null,
  units,
  precision,
  sheet,
  layers = [],
  linetypes = [],
  onSet,
  current,
  onSetCurrent,
  reveal = 0,
}: Props) {
  const t = useT();
  const id = useId();
  const [open, setOpen] = useState(true);
  const title = useRef<HTMLButtonElement>(null);
  const [folded, setFolded] = useState(storedFolded);
  const fold = (id: string) =>
    setFolded((was) => {
      const next = was.includes(id) ? was.filter((x) => x !== id) : [...was, id];
      try {
        localStorage.setItem(FOLDED, JSON.stringify(next));
      } catch {
        // Not remembered; the section is folded for this session.
      }
      return next;
    });
  useEffect(() => {
    if (reveal === 0) return;
    setOpen(true);
    title.current?.focus();
  }, [reveal]);
  const m = metresPerUnit(units);
  /** "ByLayer", "ByBlock", "Default" and "None" in the window's language. */
  const byName = (v: string) =>
    ({ ByLayer: t("props.byLayer"), ByBlock: t("props.byBlock"), None: t("props.noColor") })[v] ?? v;
  const weight = (v: number) => weightLabel(v, t);
  const editable = !!entity && !!onSet && entity.chain.length === 0 && !entity.xref;
  // Fields start again from the core's value when the entity or its values change.
  const key = entity ? JSON.stringify(entity) : current ? JSON.stringify(current) : "";

  const multiple = many && many.length >= 2 ? many : null;
  const general: Row[] = [];
  const geometry: Row[] = [];
  const selected: Row[] = [];
  const attributes = !multiple && entity ? entity.attributes : [];
  /** A row; `text` rows read in their own direction, the rest left to right. */
  const into = (list: Row[]) => (label: string, value: ReactNode, text = false) => list.push([label, value, text]);
  const row = into(general);
  const types = ["ByLayer", "ByBlock", ...linetypes];
  const weights = [-1, -2, -3, ...LINEWEIGHTS];
  const weightSelect = (value: number, onPick: (w: number) => void) => (
    <select aria-label={t("props.lineweight")} value={String(value)} onChange={(e) => onPick(Number(e.target.value))}>
      {weights.map((w) => (
        <option key={w} value={String(w)}>
          {weight(w)}
        </option>
      ))}
    </select>
  );
  /** A length field in metres, sent in drawing units. */
  const lengthField = (label: string, v: number, prop: EntityProp) => (
    <Field
      label={t(label)}
      value={(v * m).toFixed(precision)}
      onCommit={(s) => {
        const n = typedNumber(s);
        if (n !== null) onSet?.(prop, n / m);
      }}
    />
  );
  const numberField = (label: string, v: number, prop: EntityProp) => (
    <Field
      label={t(label)}
      value={String(Number(v.toFixed(6)))}
      onCommit={(s) => {
        const n = typedNumber(s);
        if (n !== null) onSet?.(prop, n);
      }}
    />
  );
  const swatch = (rgb: [number, number, number]) => (
    <span className="swatch" style={{ background: `rgb(${rgb.join(",")})` }} />
  );
  /** The Geometry rows of `entity`: a value that is edited today becomes its field, the rest are
   *  read-only values that a click copies. */
  const geometryOf = (e: EntityInfo, edit: boolean) => {
    const put = into(geometry);
    for (const r of geometryRows(e, units, precision, t)) {
      const textual = e.kind === "Text" || e.kind === "MText";
      const field: [string, ReactNode, boolean?] | null = !edit
        ? null
        : r.key === "radius" && e.radius !== null
          ? ["props.radiusM", lengthField("props.radiusM", e.radius, "radius")]
          : r.key === "closed" && e.closed != null
            ? [
                "props.closed",
                <input
                  type="checkbox"
                  aria-label={t("props.closed")}
                  checked={e.closed}
                  onChange={(ev) => onSet?.("closed", ev.currentTarget.checked)}
                />,
              ]
            : r.key === "width" && e.global_width != null
              ? ["props.widthM", lengthField("props.widthM", e.global_width, "global_width")]
              : r.key === "scale" && e.kind === "Insert" && e.scale != null
                ? ["props.scale", numberField("props.scale", e.scale, "insert_scale")]
                : r.key === "rotation" && e.rotation != null
                  ? [
                      "props.rotationDeg",
                      numberField("props.rotationDeg", e.rotation, e.kind === "Insert" ? "insert_rotation" : "text_rotation"),
                    ]
                  : r.key === "height" && e.height !== null
                    ? ["props.heightM", lengthField("props.heightM", e.height, "text_height")]
                    : r.key === "text" && textual
                      ? [
                          "props.text",
                          <Field label={t("props.text")} dir="auto" value={e.plain_text ?? e.text ?? ""} onCommit={(v) => onSet?.("text", v)} />,
                          true,
                        ]
                      : r.key === "viewportScale" && e.viewport_scale != null
                        ? ["props.viewportScale", numberField("props.viewportScale", e.viewport_scale, "viewport_scale")]
                        : r.key === "viewportCentre" && e.viewport_centre
                          ? [
                              "props.viewportCentre",
                              <Field
                                label={t("props.viewportCentre")}
                                value={`${Number(e.viewport_centre[0].toFixed(6))}, ${Number(e.viewport_centre[1].toFixed(6))}`}
                                onCommit={(s) => {
                                  const parts = normalizeDigits(s).split(",").map(typedNumber);
                                  if (parts.length === 2 && parts[0] !== null && parts[1] !== null) {
                                    onSet?.("viewport_centre", [parts[0], parts[1]]);
                                  }
                                }}
                              />,
                            ]
                          : null;
      if (field) put(...field);
      else put(r.label, <Value row={r} />, r.prose);
    }
  };

  if (multiple) {
    const sum = summarise(multiple);
    const varies = t("props.varies");
    into(general)(
      "props.layer",
      sum.layerShort === null ? (
        varies
      ) : (
        <span title={sum.layer ?? undefined}>{sum.layerShort}</span>
      ),
      true,
    );
    row(
      "props.color",
      sum.color === null ? (
        varies
      ) : (
        <>
          {sum.colorRgb && swatch(sum.colorRgb)}
          {byName(sum.color)}
        </>
      ),
    );
    row("props.linetype", sum.linetype === null ? varies : byName(sum.linetype), true);
    row("props.lineweight", sum.lineweight === null ? varies : weight(sum.lineweight));
    row("props.sheet", sheet, true);
    for (const r of selectionRows(sum, units, precision, t)) into(selected)(r.label, <Value row={r} />, r.prose);
  } else if (entity && editable && onSet) {
    row("props.kind", entityKind(entity.kind, t));
    row("props.layer", <Choice label={t("props.layer")} value={entity.layer} names={layers} onPick={(v) => onSet("layer", v)} />);
    row(
      "props.color",
      <>
        {swatch(entity.color_rgb)}
        <Field label={t("props.color")} value={entity.color} onCommit={(v) => v.trim() && onSet("color", v.trim())} />
      </>,
    );
    row("props.linetype", <Choice label={t("props.linetype")} value={entity.linetype} names={types} onPick={(v) => onSet("linetype", v)} />);
    row("props.lineweight", weightSelect(entity.lineweight, (w) => onSet("lineweight", w)));
    row("props.sheet", sheet, true);
    geometryOf(entity, true);
  } else if (entity) {
    row("props.kind", entityKind(entity.kind, t));
    row("props.layer", <span title={entity.layer}>{entity.layer_short}</span>, true);
    row("props.color", <>{swatch(entity.color_rgb)}{byName(entity.color)}</>);
    row("props.linetype", byName(entity.linetype), true);
    row("props.lineweight", weight(entity.lineweight));
    if (entity.block_path.length > 0) row("props.path", entity.block_path.join(" › "), true);
    if (entity.xref) row("props.xref", entity.xref, true);
    row("props.sheet", sheet, true);
    geometryOf(entity, false);
  } else if (current && onSetCurrent) {
    const set = (change: Partial<Current>) => onSetCurrent({ ...current, ...change });
    row("props.layer", <Choice label={t("props.layer")} value={current.layer} names={layers} onPick={(layer) => set({ layer })} />);
    row("props.color", <Field label={t("props.color")} value={current.color} onCommit={(v) => v.trim() && set({ color: v.trim() })} />);
    row("props.linetype", <Choice label={t("props.linetype")} value={current.linetype} names={types} onPick={(linetype) => set({ linetype })} />);
    row("props.lineweight", weightSelect(current.lineweight, (lineweight) => set({ lineweight })));
  }

  const sections: SectionData[] = [
    { id: "selection", title: "props.section.selection", rows: selected },
    { id: "general", title: "props.section.general", rows: general },
    { id: "geometry", title: "props.section.geometry", rows: geometry },
  ].filter((section) => section.rows.length > 0);
  const empty = sections.length === 0 && attributes.length === 0;

  return (
    <section className="properties" aria-labelledby={`${id}-title`}>
      <h2 className="heading" id={`${id}-title`}>
        <button
          type="button"
          ref={title}
          aria-expanded={open}
          aria-controls={`${id}-body`}
          onClick={() => setOpen((o) => !o)}
        >
          {t("props.title")}
        </button>
      </h2>
      {open && (
        <div id={`${id}-body`}>
          {!entity && !multiple && !empty && <p className="muted">{t("props.current")}</p>}
          {!entity && !multiple && empty ? (
            <p className="muted">{t("props.none")}</p>
          ) : (
            <div key={key}>
              {sections.map((section) => (
                <Section key={section.id} id={`${id}-${section.id}`} title={t(section.title)} folded={folded.includes(section.id)} onFold={() => fold(section.id)}>
                  <dl>
                    {section.rows.map(([label, value, text]) => (
                      <div key={label}>
                        <dt>{t(label)}</dt>
                        <dd dir={text ? "auto" : "ltr"}>{value}</dd>
                      </div>
                    ))}
                  </dl>
                </Section>
              ))}
              {attributes.length > 0 && (
                <Section
                  id={`${id}-attributes`}
                  className="attributes"
                  title={t("props.attributes")}
                  folded={folded.includes("attributes")}
                  onFold={() => fold("attributes")}
                >
                  <dl>
                    {attributes.map(([tag, value], i) => (
                      <div key={i}>
                        <dt dir="auto">{tag}</dt>
                        <dd dir="auto">{value}</dd>
                      </div>
                    ))}
                  </dl>
                </Section>
              )}
            </div>
          )}
        </div>
      )}
    </section>
  );
});
