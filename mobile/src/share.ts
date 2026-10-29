/**
 * Native share sheet for a file the webview holds in memory.
 *
 * In WKWebView / Android WebView `<a download>` does nothing useful and
 * `window.print()` is a no-op, so the bytes go to the OS instead: iOS's sheet
 * offers Save to Files, AirDrop, Mail and (for a PDF) Print, Android's chooser
 * lists the apps that take the type. The bytes are first written to the app's
 * cache by the `write_share_file` command (src-tauri/src/share_file.rs), then
 * the `share` plugin (mobile/tauri-plugin-share) opens the sheet on that path.
 */
import { invoke } from '@tauri-apps/api/core'

export interface ShareResult {
  /**
   * True when the user picked an action, false when they dismissed the sheet.
   * Android reports true as soon as its chooser opens.
   */
  completed: boolean
}

/**
 * Share `bytes` as a file named `filename`. Resolves when the sheet closes
 * (iOS) or opens (Android); rejects if the file can't be written or no sheet
 * can be shown.
 */
export async function shareFile(
  bytes: Uint8Array,
  filename: string,
  mimeType: string,
): Promise<ShareResult> {
  // The bytes go as the raw invoke body, not a JSON number array. Header
  // values must be ASCII, so the name is percent-encoded; Rust decodes and
  // sanitises it.
  const path = await invoke<string>('write_share_file', bytes, {
    headers: { 'x-share-filename': encodeURIComponent(filename) },
  })
  return invoke<ShareResult>('plugin:share|share_file', { payload: { path, mimeType } })
}
