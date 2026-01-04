/**
 * Group Type Definitions
 * User group management interfaces
 */

import type { UserInfo } from './user'
import type { Device } from './device'

// Simplified user info returned by group APIs (matches backend UserInfoWithAvatar)
export interface GroupMember {
  uuid: string
  name: string
  avatar_url?: string | null
  avatar_thumb?: string | null
}

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
  device_count: number
}

export interface GroupWithMembers extends Group {
  members: UserInfo[]
}

// Full group details with members and devices (for detail view)
export interface GroupDetails extends Group {
  external_id?: string | null
  external_source?: string | null
  group_type?: string | null
  mail_enabled: boolean
  security_enabled: boolean
  last_synced_at?: string | null
  sync_enabled: boolean
  members: GroupMember[]
  devices: Device[]
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
