/**
 * Haptic feedback via `@tauri-apps/plugin-haptics`. Deliberately NOT
 * exported from `index.ts`: the frontend imports this as
 * `@nosdesk/mobile/haptics` (the `"./*"` exports map) so a haptic tick
 * never pulls the bootstrap graph into its chunk.
 *
 * The plugin only does anything on iOS/Android; on the desktop dev
 * shell the invoke fails and we swallow it — haptics are polish.
 */
export async function impactLight(): Promise<void> {
  try {
    const { impactFeedback } = await import('@tauri-apps/plugin-haptics')
    await impactFeedback('light')
  } catch {
    // Desktop shell / plugin missing: silent no-op.
  }
}
