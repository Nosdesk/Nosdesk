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
import { platform } from '@tauri-apps/plugin-os'

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
  // iOS takes the bytes as the raw invoke body. Android's IPC has no raw body
  // and would turn a Uint8Array into a JSON array of numbers (four or more
  // bytes per byte), so it gets base64, a third larger. Header values must be
  // ASCII, so the name is percent-encoded; Rust decodes and sanitises it.
  const body = platform() === 'android' ? { base64: toBase64(bytes) } : bytes
  const path = await invoke<string>('write_share_file', body, {
    headers: { 'x-share-filename': encodeURIComponent(filename) },
  })
  return invoke<ShareResult>('plugin:share|share_file', { payload: { path, mimeType } })
}

// btoa takes a binary string; build it in chunks so a large file doesn't
// overflow the argument limit of String.fromCharCode.
function toBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  }
  return btoa(binary)
}
