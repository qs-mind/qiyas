import { isWorkspaceBusy, uiError } from "../i18n/errors";
import { notifyError } from "../ui/notices";

/** Refreshes keep their last data during a tool's transaction and retry on the next change.
 * Unlike a user command, a refresh never clears notices or consumes an Undo offer. */
export async function refreshInBackground(step: () => Promise<void>) {
  try {
    await step();
  } catch (error) {
    if (!isWorkspaceBusy(error)) notifyError(uiError(error));
  }
}
