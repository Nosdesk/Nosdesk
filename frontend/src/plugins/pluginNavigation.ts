/**
 * Navigation for `api.ui.openRecord` / `api.ui.openPage`.
 *
 * Only the app's own route builders, never a path a plugin supplies. Every
 * target is an authenticated route, so the workspace guard adds the
 * `/:workspace?` slug in path mode the same way it does for the app's links.
 * The plugin API validates the reference before it gets here.
 *
 * Kept apart from `api.ts` (which imports it lazily) because the router pulls in
 * every view, which the plugin API's unit tests don't load.
 */
import router from '@/router';
import { ticketPathForId } from '@/utils/ticketNumbers';
import { pluginPagePath } from './pluginPage';

export type PluginNavigationTarget =
  | { type: 'ticket'; id: number }
  | { type: 'asset'; id: number }
  | { type: 'user'; uuid: string }
  | { type: 'page'; pluginUuid: string; componentName: string };

/** Start navigating to `target`. Doesn't wait for the navigation to finish: the
 * destination may unmount the plugin that asked, and its bridge with it. */
export function navigateForPlugin(target: PluginNavigationTarget): void {
  let push: Promise<unknown>;
  switch (target.type) {
    case 'ticket':
      push = router.push(ticketPathForId(target.id));
      break;
    case 'asset':
      push = router.push({ name: 'asset-view', params: { id: String(target.id) } });
      break;
    case 'user':
      push = router.push({ name: 'user-profile', params: { uuid: target.uuid } });
      break;
    case 'page':
      push = router.push(pluginPagePath(target.pluginUuid, target.componentName));
      break;
  }
  push.catch(() => {
    // A guard redirect or a cancelled navigation is not the plugin's concern.
  });
}
