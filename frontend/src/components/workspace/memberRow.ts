import type { WorkspaceRole } from '@nosdesk/core/types/workspace'

/**
 * A `workspace_members` row joined with the identity the sync pool
 * holds for that user, plus the facts the row's controls need.
 *
 * The members endpoint returns uuids only; names, emails and avatars
 * are resolved client-side from the pool's workspace-grouped `user`
 * aggregate. Shared between `WorkspaceMembersView` (which builds it)
 * and `MemberRoleMenu` (which renders from it), so the menu takes one
 * prop rather than restating every field.
 */
export interface WorkspaceMemberRow {
  /** Row identity for `ListPageLayout`, whose `rowKey` reads `.id` and
   *  silently falls back to the array index when it's absent. Index
   *  keys make the mobile TransitionGroup animate the wrong row on
   *  removal, so this mirrors `user_uuid` to keep keys stable. */
  id: string
  user_uuid: string
  role: WorkspaceRole
  invited_at: string
  accepted_at: string | null
  /** Pool-resolved display name, falling back to email then a short uuid. */
  name: string
  email: string
  avatar: string | null
  isYou: boolean
  /** Derived once here rather than re-deriving `accepted_at ? .. : ..`
   *  at each of the three places that render or sort by it. */
  status: 'active' | 'pending'
  statusLabel: string
  /** The caller's tier can manage this row and it isn't the sole owner. */
  editable: boolean
  /** Why the row is locked, when there's a reason worth surfacing
   *  (the last-owner rule). Empty for "simply outranks you". */
  lockedHint: string
}
