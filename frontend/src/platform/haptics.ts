/**
 * Web-safe haptics facade. On the web this is a guaranteed no-op and
 * the `@nosdesk/mobile` chunk is never loaded; under Tauri it lazily
 * pulls the haptics module (subpath import, so the bootstrap graph
 * isn't dragged in) and fires the native impact. Fire-and-forget by
 * design — gesture code must never await feedback.
 */
import { isTauriRuntime } from './index'

export function hapticImpactLight(): void {
  if (!isTauriRuntime()) return
  void import('@nosdesk/mobile/haptics')
    .then((m) => m.impactLight())
    .catch(() => {
      // Plugin absent (desktop dev shell) — feedback is polish, not signal.
    })
}
