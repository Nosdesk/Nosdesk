/**
 * Who a user picker offers. `assignee` is the people who can be assigned a
 * ticket; `requester` is anyone. The ticket sidebar's picker
 * (`useUserPicker`) and the user selection modal both load through here,
 * so a bulk assign offers the same people as a single one.
 */
import userService from '@/services/userService'
import type { RecentScope } from '@/stores/recentUsers'
import { isStaffWorkspaceRole, type PlatformRole, type UserInfo } from '@nosdesk/core/types/user'
import type { WorkspaceRole } from '@nosdesk/core/types/workspace'

export type PickerScope = RecentScope

const PAGE_SIZE = 50

type RoleFields = { platform_role?: PlatformRole | null; workspace_role?: WorkspaceRole | null }

/** Who can be assigned a ticket: a platform admin, or a workspace owner,
 *  admin or agent. The server owns the rule (`assignees::is_assignable`)
 *  and filters the list by it; this copy only guards rows the picker holds
 *  without asking, a cached user or a recents entry from before someone's
 *  role changed. */
function isAssignableUser(u: RoleFields): boolean {
  return u.platform_role === 'platform_admin' || isStaffWorkspaceRole(u.workspace_role)
}

/** Whether a cached or recent row `u` belongs in a picker of `type`. */
export function isEligibleForType(type: PickerScope, u: RoleFields): boolean {
  return type === 'requester' || isAssignableUser(u)
}

/** The first page of people a picker of `type` offers, matching
 *  `search`, filtered and paged by the server. */
export async function fetchEligibleUsers(
  type: PickerScope,
  search: string = '',
  pageSize: number = PAGE_SIZE,
): Promise<UserInfo[]> {
  const response = await userService.getPaginatedUsers({
    page: 1,
    pageSize,
    search,
    sortField: 'name',
    sortDirection: 'asc',
    ...(type === 'assignee' ? { assignable: true } : {}),
  })
  return response.data
}
