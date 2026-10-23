// Seals and unseals the licensed Thmanyah Sans fonts the window uses.
// The repo carries only the sealed file (assets/fonts/thmanyah-sans.sealed), encrypted with
// AES-256-GCM; the key lives in the THMANYAH_FONTS_KEY repository secret and, on the founder's
// computer, in ~/.config/qs-mind/thmanyah-fonts.key. Without the key the sealed file is useless.
//
//   node scripts/fonts-seal.mjs seal <key file>   # src/fonts/thmanyah/*.otf -> sealed file
//   THMANYAH_FONTS_KEY=… node scripts/fonts-seal.mjs unseal   # sealed file -> src/fonts/thmanyah/
import { createCipheriv, createDecipheriv, randomBytes, scryptSync } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const sealed = join(root, "assets/fonts/thmanyah-sans.sealed");
const fonts = join(root, "src/fonts/thmanyah");
const files = ["thmanyahsans-Regular.otf", "thmanyahsans-Medium.otf", "thmanyahsans-Bold.otf"];
const MAGIC = Buffer.from("QSF1");

const keyOf = (secret, salt) => scryptSync(secret.trim(), salt, 32);

function seal(keyFile) {
  const parts = [];
  for (const name of files) {
    const data = readFileSync(join(fonts, name));
    const head = Buffer.alloc(6);
    head.writeUInt16BE(Buffer.byteLength(name), 0);
    head.writeUInt32BE(data.length, 2);
    parts.push(head, Buffer.from(name), data);
  }
  const salt = randomBytes(16);
  const iv = randomBytes(12);
  const cipher = createCipheriv("aes-256-gcm", keyOf(readFileSync(keyFile, "utf8"), salt), iv);
  const body = Buffer.concat([cipher.update(Buffer.concat(parts)), cipher.final()]);
  mkdirSync(dirname(sealed), { recursive: true });
  writeFileSync(sealed, Buffer.concat([MAGIC, salt, iv, cipher.getAuthTag(), body]));
  console.log(`Sealed ${files.length} font files into ${sealed}`);
}

function unseal(secret) {
  const all = readFileSync(sealed);
  if (!all.subarray(0, 4).equals(MAGIC)) throw new Error("not a sealed font file");
  const salt = all.subarray(4, 20);
  const iv = all.subarray(20, 32);
  const tag = all.subarray(32, 48);
  const decipher = createDecipheriv("aes-256-gcm", keyOf(secret, salt), iv);
  decipher.setAuthTag(tag);
  const plain = Buffer.concat([decipher.update(all.subarray(48)), decipher.final()]);
  mkdirSync(fonts, { recursive: true });
  let at = 0;
  let count = 0;
  while (at < plain.length) {
    const nameLen = plain.readUInt16BE(at);
    const dataLen = plain.readUInt32BE(at + 2);
    const name = plain.subarray(at + 6, at + 6 + nameLen).toString();
    if (!files.includes(name)) throw new Error(`unexpected file in sealed fonts: ${name}`);
    const start = at + 6 + nameLen;
    writeFileSync(join(fonts, name), plain.subarray(start, start + dataLen));
    at = start + dataLen;
    count += 1;
  }
  console.log(`Unsealed ${count} font files into ${fonts}`);
}

const [mode, keyFile] = process.argv.slice(2);
if (mode === "seal" && keyFile) seal(keyFile);
else if (mode === "unseal" && process.env.THMANYAH_FONTS_KEY) unseal(process.env.THMANYAH_FONTS_KEY);
else {
  console.error("usage: node scripts/fonts-seal.mjs seal <key file> | THMANYAH_FONTS_KEY=… … unseal");
  process.exit(2);
}
