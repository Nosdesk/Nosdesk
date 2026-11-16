/** True inside the Tauri webview (Tauri 2 injects `window.isTauri`). */
export function isTauriRuntime(): boolean {
  return typeof globalThis !== 'undefined' && !!(globalThis as { isTauri?: boolean }).isTauri
}
