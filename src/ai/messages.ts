/** The assistant's transcript, built from the agent's events by a pure reducer. */
import type { AgentEvent, AiUi } from "../api";

export type ToolStatus = "running" | "done" | "error";

export type ToolCardItem = {
  kind: "tool";
  id: string;
  name: string;
  input: unknown;
  status: ToolStatus;
  /** One line from the tool's result; empty while it runs. */
  summary: string;
  /** Entities the result is about, to highlight. */
  handles: number[];
  ui: AiUi | null;
  /** The tool changed the take-off (an undo step). */
  mutating: boolean;
  /** The user pressed Undo on this card. */
  undone: boolean;
  /** Undo was refused: the take-off changed since this card's change. */
  stale: boolean;
};

/** What went with a user's message besides their words: the selection or area they pointed at
 *  (a short label), and the pictures they attached (data URLs, for thumbnails). */
export type Attached = { pointed?: string[]; images?: string[] };

export type Item =
  | ({ kind: "user"; text: string } & Attached)
  /** Streamed: `finished` once anything else follows it or the turn ends. */
  | { kind: "assistant"; text: string; finished: boolean }
  | { kind: "thinking"; text: string }
  | ToolCardItem
  | { kind: "refused"; category: string | null; text: string }
  | { kind: "stopped"; reason: string };

export type Transcript = { items: Item[] };

/** The agent's events, plus what the window does itself. */
export type Action =
  | AgentEvent
  | ({ type: "user"; text: string } & Attached)
  /** The message could not be sent: take it back. */
  | { type: "unsend" }
  /** The turn has ended (`ai:done`). */
  | { type: "end" }
  | { type: "undone"; id: string }
  | { type: "undo_stale"; id: string }
  | { type: "clear" };

export const EMPTY: Transcript = { items: [] };

/** Why the agent stops a conversation that has grown too long (`qiyas_ai::agent::TOO_LONG`). */
export const TOO_LONG = "conversation is long, start a new one";

/** The items with a streaming assistant message, if last, marked finished. */
function finishText(items: Item[]): Item[] {
  const last = items.at(-1);
  if (last?.kind !== "assistant" || last.finished) return items;
  return [...items.slice(0, -1), { ...last, finished: true }];
}

function updateCard(items: Item[], id: string, patch: (card: ToolCardItem) => ToolCardItem): Item[] {
  return items.map((i) => (i.kind === "tool" && i.id === id ? patch(i) : i));
}

export function reduce(state: Transcript, action: Action): Transcript {
  const items = state.items;
  const last = items.at(-1);
  switch (action.type) {
    case "user": {
      const { type: _, ...item } = action;
      return { items: [...finishText(items), { ...item, kind: "user" }] };
    }
    case "unsend":
      return last?.kind === "user" ? { items: items.slice(0, -1) } : state;
    case "text":
      if (last?.kind === "assistant" && !last.finished) {
        return { items: [...items.slice(0, -1), { ...last, text: last.text + action.text }] };
      }
      return { items: [...items, { kind: "assistant", text: action.text, finished: false }] };
    case "thinking":
      // The API sends thinking with its text left out unless asked; nothing to show then.
      if (!action.text) return state;
      if (last?.kind === "thinking") {
        return { items: [...items.slice(0, -1), { kind: "thinking", text: last.text + action.text }] };
      }
      return { items: [...finishText(items), { kind: "thinking", text: action.text }] };
    case "tool_call":
      return {
        items: [
          ...finishText(items),
          {
            kind: "tool",
            id: action.id,
            name: action.name,
            input: action.input,
            status: "running",
            summary: "",
            handles: [],
            ui: null,
            mutating: action.mutating,
            undone: false,
            stale: false,
          },
        ],
      };
    case "tool_result":
      return {
        items: updateCard(finishText(items), action.id, (card) => ({
          ...card,
          status: action.is_error ? "error" : "done",
          summary: action.summary,
          handles: action.handles,
          ui: action.ui,
        })),
      };
    case "refused":
      return {
        items: [...finishText(items), { kind: "refused", category: action.category, text: action.text }],
      };
    case "stopped":
      return { items: [...finishText(items), { kind: "stopped", reason: action.reason }] };
    case "done":
      return { items: finishText(items) };
    case "end":
      // A turn that failed may leave a card running; it never will finish now.
      return {
        items: finishText(items).map((i) =>
          i.kind === "tool" && i.status === "running" ? { ...i, status: "error" } : i,
        ),
      };
    case "undone":
      return { items: updateCard(items, action.id, (card) => ({ ...card, undone: true })) };
    case "undo_stale":
      return { items: updateCard(items, action.id, (card) => ({ ...card, stale: true })) };
    case "clear":
      return EMPTY;
  }
}

/** The card whose change is the assistant's newest still in the take-off: the newest done,
 *  mutating card not undone yet. Undoing it makes the one before it the next. */
export function undoableCard(t: Transcript): ToolCardItem | null {
  for (let i = t.items.length - 1; i >= 0; i--) {
    const item = t.items[i];
    if (item.kind === "tool" && item.mutating && item.status === "done" && !item.undone) return item;
  }
  return null;
}

/** The transcript ends where only a new conversation helps: a refusal, or one grown too long. */
export function startNewOffered(t: Transcript): boolean {
  const last = t.items.at(-1);
  return last?.kind === "refused" || (last?.kind === "stopped" && last.reason === TOO_LONG);
}
