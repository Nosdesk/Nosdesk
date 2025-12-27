/**
 * Group Type Definitions
 * User group management interfaces
 */

import type { UserInfo } from './user'

export interface Group {
  id: number
  uuid: string
  name: string
  description?: string | null
  color?: string | null
  created_at: string
  updated_at: string
  created_by?: string | null
}

export interface GroupWithMemberCount extends Group {
  member_count: number
}

export interface GroupWithMembers extends Group {
  members: UserInfo[]
}

export interface CreateGroupRequest {
  name: string
  description?: string
  color?: string
}

export interface UpdateGroupRequest {
  name?: string
  description?: string
  color?: string
}

export interface SetGroupMembersRequest {
  member_uuids: string[]
}

export interface SetUserGroupsRequest {
  group_ids: number[]
}
