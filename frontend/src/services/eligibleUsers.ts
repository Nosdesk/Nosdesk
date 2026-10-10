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
 *  admin or agent. The one client copy of the server's rule
 *  (`repository/assignees.rs::is_assignable`); every assignee list in
 *  the app filters through it, so none can offer someone the server
 *  refuses. */
function isAssignableUser(u: RoleFields): boolean {
  return u.platform_role === 'platform_admin' || isStaffWorkspaceRole(u.workspace_role)
}

/** The server-side role filter for the same people (`/users/paginated`
 *  reads `admin,technician` as platform admin or owner/admin/agent). */
const ROLE_FILTER: Record<PickerScope, string | undefined> = {
  assignee: 'admin,technician',
  requester: undefined,
}

/** Whether `u` belongs in a picker of `type`. The server already filters
 *  by role; this also drops a stale cached row or a recents entry from
 *  before the rule tightened. */
export function isEligibleForType(type: PickerScope, u: RoleFields): boolean {
  return type === 'requester' || isAssignableUser(u)
}

/** The first page of people a picker of `type` offers, matching
 *  `search`. */
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
    role: ROLE_FILTER[type],
  })
  return response.data.filter((u) => isEligibleForType(type, u))
}
