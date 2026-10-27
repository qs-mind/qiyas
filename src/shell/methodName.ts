/** Measurement method names as people see them. The professional body that publishes a method
 *  is never named in the window (founder's ruling, 26 October 2026): "POMI", "NRM2", not
 *  "POMI (RICS)". Older settings and drawings keep the longer name in their data; it is shown
 *  and saved without the body. */

const BODY = /\s*[([]\s*RICS\s*[)\]]|\bRICS\b\s*/gi;

/** `POMI (RICS)` -> `POMI`. Text with no body in it comes back as it was. */
export function plainMethod(name: string): string;
export function plainMethod(name: string | null | undefined): string | null;
export function plainMethod(name: string | null | undefined): string | null {
  if (name == null) return null;
  const cleaned = name.replace(BODY, "").trim();
  return cleaned || name.trim();
}

/** Any longer text (an answer, a note): the body's name taken out, nothing else changed. */
export function withoutBody(text: string): string {
  return text.replace(BODY, "");
}
