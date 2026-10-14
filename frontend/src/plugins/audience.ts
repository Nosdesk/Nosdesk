/**
 * Per-component `audience` (manifest `components.<name>.audience`).
 *
 * The loader's registration getters apply this, so every slot, nav link,
 * action, plugin page and modal sees one filtered set. Applied at READ time,
 * not at load: plugins can load before the viewer's workspace role resolves,
 * and the role changes on a workspace switch. Reading the auth store's
 * computeds here keeps every consumer's `computed` reactive to both.
 *
 * Visibility only, not a security boundary: plugin permissions are enforced on
 * the client bridge and the viewer's own API permissions still apply.
 */
import type { PluginAudience } from '@nosdesk/core/types/plugin';
import { useAuthStore } from '@/stores/auth';

export interface ViewerTier {
  /** Workspace agent and up (backend `can_handle_tickets`). */
  isStaff: boolean;
  /** Workspace admin and up. */
  isAdmin: boolean;
}

/** True when a viewer of `viewer`'s tier may see a component of `audience`.
 *  An unrecognised value hides the component (the backend rejects it at
 *  install, so this only guards a manifest from a newer host). */
export function meetsAudience(audience: PluginAudience | undefined, viewer: ViewerTier): boolean {
  switch (audience ?? 'all') {
    case 'all':
      return true;
    case 'staff':
      return viewer.isStaff;
    case 'admin':
      return viewer.isAdmin;
    default:
      return false;
  }
}

/** The signed-in viewer's tier, from the auth store's role computeds. */
export function currentViewerTier(): ViewerTier {
  const auth = useAuthStore();
  return { isStaff: auth.isTechnician, isAdmin: auth.isAdmin };
}
