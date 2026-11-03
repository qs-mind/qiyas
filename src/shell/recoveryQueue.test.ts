import { expect, test } from "vitest";
import type { Recoverable } from "../api";
import { atLaunch, drop, enqueue, forgetThen, laterOpens, localTime, offered, refusedBy, type RecoveryAsk } from "./recoveryQueue";

const item = (drawing: string, crashed = false): Recoverable => ({
  drawing,
  autosave: `${drawing}.sv$`,
  saved_at: 1,
  drawing_at: 2,
  crashed,
  locked_elsewhere: null,
  older_copies: [],
});

const copy = (drawing: string, stamp: string, saved_at: number) => ({
  path: `${drawing}.sv$.crash-${stamp}`,
  saved_at,
  kind: "crash" as const,
});

test("crashed drawings are asked about first, each kind in the order found", () => {
  const q = enqueue([], [item("/a.dwg"), item("/b.dwg", true), item("/c.dwg"), item("/d.dwg", true)], null, true);
  expect(q[0].items.map((i) => i.drawing)).toEqual(["/b.dwg", "/d.dwg", "/a.dwg", "/c.dwg"]);
});

test("a drawing's own question keeps its place in line, crashed or not (Open all opens in order)", () => {
  let q: RecoveryAsk[] = [];
  q = enqueue(q, [item("/a.dwg")], "/a.dwg");
  q = enqueue(q, [item("/b.dwg", true)], "/b.dwg");
  expect(q.map((r) => r.then)).toEqual(["/a.dwg", "/b.dwg"]);
});

test("each drawing's question waits its turn; the launch one goes first", () => {
  let q: RecoveryAsk[] = [];
  q = enqueue(q, [item("/a.dwg")], "/a.dwg");
  q = enqueue(q, [item("/b.dwg")], "/b.dwg");
  q = enqueue(q, [item("/c.dwg")], null, true);
  expect(q.map((r) => r.then)).toEqual([null, "/a.dwg", "/b.dwg"]);
  expect(new Set(q.map((r) => r.id)).size).toBe(3);
});

test("a drawing dealt with leaves the question it was asked in, not the first in line; an empty one goes", () => {
  let q: RecoveryAsk[] = [];
  q = enqueue(q, [item("/a.dwg"), item("/b.dwg")], null);
  q = enqueue(q, [item("/c.dwg")], "/c.dwg");
  const [first, second] = q;
  // The click came from the second question (the launch one was put ahead meanwhile).
  expect(drop(q, second.id, "/c.dwg")).toEqual([first]);
  q = drop(q, first.id, "/a.dwg");
  expect(q[0].items.map((i) => i.drawing)).toEqual(["/b.dwg"]);
  q = drop(q, first.id, "/b.dwg");
  expect(q.map((r) => r.then)).toEqual(["/c.dwg"]);
  expect(drop([], 1, "/x.dwg")).toEqual([]);
});

test("a drawing dealt with is no longer what Later opens (its question may hold others)", () => {
  let q = enqueue([], [item("/x.dwg"), item("/y.dwg")], null, true);
  q = enqueue(q, [item("/x.dwg")], "/x.dwg");
  expect(q[0].then).toBe("/x.dwg");
  q = drop(q, q[0].id, "/x.dwg");
  expect(q[0].items.map((i) => i.drawing)).toEqual(["/y.dwg"]);
  expect(laterOpens(q[0])).toBeNull();
});

test("an older copy dealt with leaves its row; the row goes when nothing is left in it", () => {
  const a = { ...item("/a.dwg"), autosave: null, saved_at: null, older_copies: [copy("/a.dwg", "1", 5), copy("/a.dwg", "2", 4)] };
  let q = enqueue([], [a], "/a.dwg");
  q = drop(q, q[0].id, "/a.dwg", a.older_copies[0].path);
  expect(q[0].items[0].older_copies.map((c) => c.path)).toEqual([a.older_copies[1].path]);
  expect(q[0].then).toBe("/a.dwg");
  q = drop(q, q[0].id, "/a.dwg", a.older_copies[1].path);
  expect(q).toEqual([]);
});

test("times are written in the app's language, whatever the system's", () => {
  // Noon UTC: the 12th of October 2026 in every time zone.
  const at = Date.UTC(2026, 9, 12, 12, 0) / 1000;
  expect(localTime(at, "en")).toMatch(/Oct.*2026/);
  // Arabic: its own date order and its own AM/PM (ص / م), never English words.
  expect(localTime(at, "ar")).toMatch(/[\u0600-\u06FF]/);
  expect(localTime(at, "ar")).not.toMatch(/Oct|AM|PM/);
});

test("Rust's refusal of a Discard gives the PID and the computer, to say in the window's language", () => {
  expect(
    refusedBy("/jobs/terminal.dwg is open in another Qiyas (process 4242 on site-office-pc): its autosave stays"),
  ).toEqual({ pid: "4242", host: "site-office-pc" });
  expect(refusedBy("Permission denied (os error 13)")).toBeNull();
});

const lockedElsewhere = (drawing: string, autosave: string | null): Recoverable => ({
  ...item(drawing),
  autosave,
  saved_at: autosave ? 1 : null,
  locked_elsewhere: { pid: 7, host: "other", started_at: 1 },
});

test("at launch only an autosave to recover is asked: not a drawing open elsewhere, a clean crash, or crash copies alone", () => {
  const found = [
    item("/a.dwg", true),
    lockedElsewhere("/b.dwg", null),
    lockedElsewhere("/c.dwg", "/c.dwg.sv$"),
    { ...item("/d.dwg"), saved_at: 3 },
    { ...item("/clean.dwg", true), autosave: null, saved_at: null },
    { ...item("/copies.dwg"), autosave: null, saved_at: null, older_copies: [copy("/copies.dwg", "1", 1)] },
  ];
  expect(atLaunch(found).map((i) => i.drawing)).toEqual(["/a.dwg", "/d.dwg"]);
});

test("Later opens the drawing asked about, unless it is open in another Qiyas (then Open anyway is the way)", () => {
  const [plain] = enqueue([], [item("/a.dwg", true)], "/a.dwg");
  expect(laterOpens(plain)).toBe("/a.dwg");
  const [locked] = enqueue([], [lockedElsewhere("/b.dwg", null)], "/b.dwg");
  expect(laterOpens(locked)).toBeNull();
  const [launch] = enqueue([], [item("/a.dwg")], null, true);
  expect(laterOpens(launch)).toBeNull();
});

test("a drawing is asked about once: a double-clicked drawing the launch question holds opens after it", () => {
  // The launch question found X first; the double click's open finds it again.
  let q = enqueue([], [item("/x.dwg"), item("/y.dwg")], null, true);
  q = enqueue(q, [item("/x.dwg")], "/x.dwg");
  expect(q).toHaveLength(1);
  expect(q[0].items.map((i) => i.drawing)).toEqual(["/x.dwg", "/y.dwg"]);
  expect(q[0].then).toBe("/x.dwg");

  // The other way round: the launch question leaves out what an open asks already.
  let r = enqueue([], [item("/x.dwg")], "/x.dwg");
  r = enqueue(r, [item("/x.dwg"), item("/y.dwg")], null, true);
  expect(r.map((a) => [a.items.map((i) => i.drawing), a.then])).toEqual([
    [["/y.dwg"], null],
    [["/x.dwg"], "/x.dwg"],
  ]);
  // Nothing left to ask: no empty question.
  expect(enqueue(r, [item("/y.dwg")], null, true)).toBe(r);
  expect(enqueue(r, [item("/x.dwg")], "/x.dwg")).toBe(r);
});

test("before an open: a crash's autosave (even older), a newer one (the same second too), a gone drawing, another Qiyas, crash copies; never an older autosave alone or a clean crash", () => {
  const found = [
    item("/older.dwg"), // saved_at 1, drawing_at 2, not crashed
    item("/crash.dwg", true),
    { ...item("/newer.dwg"), saved_at: 3 },
    { ...item("/same-second.dwg"), saved_at: 2 },
    { ...item("/gone.dwg"), drawing_at: null },
    lockedElsewhere("/held.dwg", null),
    { ...item("/copies.dwg"), autosave: null, saved_at: null, older_copies: [copy("/copies.dwg", "1", 1)] },
    { ...item("/clean.dwg", true), autosave: null, saved_at: null },
  ];
  expect(offered(found).map((i) => i.drawing)).toEqual([
    "/crash.dwg",
    "/newer.dwg",
    "/same-second.dwg",
    "/gone.dwg",
    "/held.dwg",
    "/copies.dwg",
  ]);
});

test("the main autosave dealt with: the row stays while older copies are left, as copies only", () => {
  const a = { ...item("/a.dwg", true), older_copies: [copy("/a.dwg", "1", 0)] };
  let q = enqueue([], [a, item("/b.dwg")], "/a.dwg");
  q = drop(q, q[0].id, "/a.dwg");
  const row = q[0].items.find((i) => i.drawing === "/a.dwg")!;
  expect([row.autosave, row.saved_at, row.crashed, row.older_copies.length]).toEqual([null, null, false, 1]);
  // Still asked about: Later (or the last copy's Discard) opens it.
  expect(q[0].then).toBe("/a.dwg");
  // Without older copies the row goes.
  q = drop(q, q[0].id, "/b.dwg");
  expect(q[0].items.map((i) => i.drawing)).toEqual(["/a.dwg"]);
});

test("a recovered drawing is no longer what Later opens, even while its older copies stay", () => {
  const a = { ...item("/a.dwg"), older_copies: [copy("/a.dwg", "1", 0)] };
  let q = enqueue([], [a], "/a.dwg");
  q = forgetThen(drop(q, q[0].id, "/a.dwg"), q[0].id, "/a.dwg");
  expect(q[0].items[0].older_copies).toHaveLength(1);
  expect(laterOpens(q[0])).toBeNull();
});
