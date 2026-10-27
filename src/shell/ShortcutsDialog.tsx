/** The keyboard shortcuts cheat sheet (Ctrl+/ or the SHORTCUTS command): AutoCAD's keys where Qiyas
 *  has the command, the mouse gestures, and Mizan's. */
import { useT } from "../i18n";
import { ModalDialog } from "../ui/ModalDialog";
import { keyLabel } from "../tools/shortcuts";
import { SHORTCUT_SECTIONS } from "./shortcutList";

export function ShortcutsDialog({ onClose }: { onClose(): void }) {
  const t = useT();
  return (
    <ModalDialog labelledBy="shortcuts-title" onClose={onClose} className="shortcuts">
      <div className="shortcuts-body">
        <h2 id="shortcuts-title" className="heading">{t("shortcuts.title")}</h2>
        <p className="muted">{t("shortcuts.note")}</p>
        <div className="shortcuts-grid">
          {SHORTCUT_SECTIONS.map((section) => (
            <section key={section.id} aria-labelledby={`shortcuts-${section.id}`}>
              <h3 id={`shortcuts-${section.id}`} className="heading">{t(`shortcuts.s.${section.id}`)}</h3>
              <dl>
                {section.rows.map((row) => (
                  <div key={row.id} className="shortcut-row">
                    <dt>
                      {row.keys.map((key, i) => (
                        <span key={key}>
                          {i > 0 && <span className="muted"> {t("shortcuts.or")} </span>}
                          {key.startsWith("@") ? <span className="gesture">{t(key.slice(1))}</span> : <kbd dir="ltr">{keyLabel(key)}</kbd>}
                        </span>
                      ))}
                    </dt>
                    <dd>{t(`shortcuts.r.${row.id}`)}</dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </div>
        <div className="buttons">
          <button type="button" className="primary" onClick={onClose}>{t("connect.close")}</button>
        </div>
      </div>
    </ModalDialog>
  );
}
