import { ask, message } from "@tauri-apps/plugin-dialog";

/**
 * Ask before doing something. Uses the app's native dialog: the webview's
 * window.confirm doesn't block in Tauri (it returns at once), so it must not be used.
 */
export async function confirmAction(text: string, okLabel = "OK"): Promise<boolean> {
  try {
    return await ask(text, { title: "mxKeys", kind: "warning", okLabel, cancelLabel: "Cancel" });
  } catch {
    return false; // never act without an answer
  }
}

export async function tell(text: string): Promise<void> {
  try {
    await message(text, { title: "mxKeys" });
  } catch { /* nothing else to do */ }
}
