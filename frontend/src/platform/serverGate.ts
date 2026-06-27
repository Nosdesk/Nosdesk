/**
 * First-run server gate for the native app.
 *
 * In Tauri, if no server has been chosen yet, the app shows the connect screen
 * before anything else (App.vue reads `needsServerSelection`). On the web this
 * is always false. All `@nosdesk/mobile` access is via dynamic import so the
 * web bundle never pulls Tauri code.
 */
import { ref } from 'vue'
import { isTauriRuntime } from './index'

/** True when the connect screen must show first (Tauri + no server stored). */
export const needsServerSelection = ref(false)

/** Decide on boot whether the connect screen is needed. */
export async function initServerGate(): Promise<void> {
  if (!isTauriRuntime()) return
  const { getStoredServer } = await import('@nosdesk/mobile')
  needsServerSelection.value = getStoredServer() === null
}

/** Connect to the official cloud and dismiss the gate. */
export async function selectCloud(): Promise<void> {
  const { setServer, DEFAULT_SERVER } = await import('@nosdesk/mobile')
  await setServer(DEFAULT_SERVER)
  needsServerSelection.value = false
}

/**
 * Validate a self-hosted server URL and, on success, connect to it and dismiss
 * the gate. Returns the validation error otherwise (the screen displays it).
 */
export async function connectTo(input: string): Promise<{ ok: boolean; error?: string }> {
  const { validateServer, setServer } = await import('@nosdesk/mobile')
  const result = await validateServer(input)
  if (!result.ok || !result.origin) return { ok: false, error: result.error }
  await setServer(result.origin)
  needsServerSelection.value = false
  return { ok: true }
}
