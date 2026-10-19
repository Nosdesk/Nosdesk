/**
 * What the editor does when the server refuses a document because it needs a
 * newer editor than this client's (`outdated`, close 4426).
 *
 * On the web the new build is one reload away, so the page reloads once. If
 * a reload already happened moments ago (the server still sent the old build,
 * or a chunk error reloaded just before), the editor asks instead of looping.
 * The app's editor ships inside the app, so reloading can't help: it asks for
 * an update.
 */
import { isTauriRuntime } from '@/platform'
import { reloadForNewBuild } from '@/utils/staleBuild'

/** The prompt the editor shows: reload the page, or update the app. */
export type OutdatedEditorPrompt = 'reload' | 'update-app'

/** Respond to an `outdated` refusal. Null when a reload is under way. */
export function respondToOutdatedEditor(): OutdatedEditorPrompt | null {
  if (isTauriRuntime()) return 'update-app'
  return reloadForNewBuild() ? null : 'reload'
}
