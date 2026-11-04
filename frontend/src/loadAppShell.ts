/**
 * The signed-in app's frame (AppShell.vue), loaded on demand. main.ts starts
 * it alongside the first route when that route uses the frame, so a
 * signed-in load doesn't paint without its sidebar.
 */
export const loadAppShell = () => import('./AppShell.vue')
