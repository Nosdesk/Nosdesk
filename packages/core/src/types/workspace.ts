/**
 * Workspace + workspace-membership types.
 *
 * Mirrors the JSON shape returned by:
 *   * `GET  /api/admin/workspaces`
 *   * `POST /api/admin/workspaces`
 *   * `PATCH /api/admin/workspaces/{id}`
 *   * `POST /api/admin/workspaces/{id}/archive` etc
 *   * `GET  /api/admin/workspaces/{id}/members`
 *   * `POST /api/admin/workspaces/{id}/members` (and PATCH/DELETE on /{user_uuid})
 *   * `GET  /api/me/workspaces`
 *
 * Per-workspace role values come from `workspace_members.role` —
 * see backend `WorkspaceRole` enum (Phase 4 W2).
 */

/** Per-workspace role values. */
export type WorkspaceRole = 'owner' | 'admin' | 'agent' | 'member';

/** Every role, most privileged first. Menus and pickers iterate this
 *  so the order is identical wherever roles are listed. */
export const WORKSPACE_ROLES: readonly WorkspaceRole[] = [
  'owner',
  'admin',
  'agent',
  'member',
] as const;

/** Tier ordering, mirroring the backend `WorkspaceRole` enum's `Ord`.
 *  Higher outranks lower; used for "can this caller manage that role"
 *  checks and for sorting a member list by seniority. */
export const WORKSPACE_ROLE_RANK: Record<WorkspaceRole, number> = {
  member: 0,
  agent: 1,
  admin: 2,
  owner: 3,
};

/** Server-returned workspace summary (`WorkspaceSummary` in admin_workspaces.rs). */
export interface Workspace {
  id: number;
  uuid: string;
  slug: string;
  name: string;
  plan: string;
  archived_at: string | null;
  custom_domain: string | null;
}

/** A `workspace_members` row in API shape. */
export interface WorkspaceMember {
  workspace_id: number;
  user_uuid: string;
  role: WorkspaceRole;
  invited_at: string;
  accepted_at: string | null;
}

/** Row returned by `GET /api/me/workspaces` — joins membership +
 *  workspace metadata for the frontend workspace switcher. */
export interface MyWorkspaceEntry {
  workspace_id: number;
  workspace_uuid: string;
  slug: string;
  name: string;
  custom_domain: string | null;
  role: WorkspaceRole;
  invited_at: string;
  accepted_at: string | null;
}

/** Request body for POST /api/admin/workspaces. */
export interface CreateWorkspaceRequest {
  slug: string;
  name: string;
}

/** GET /api/admin/edition — edition + workspace-limit summary. */
export interface EditionInfo {
  edition: 'community' | 'enterprise';
  self_hosted: boolean;
  max_workspaces: number;
  active_workspaces: number;
  can_create_workspace: boolean;
  license: {
    licensee: string;
    license_id: string;
    max_workspaces: number;
    expires_at: number;
  } | null;
}

/** Request body for PATCH /api/admin/workspaces/{id}. */
export interface RenameWorkspaceRequest {
  name: string;
}

/** Request body for POST /api/admin/workspaces/{id}/members. */
export interface AddMemberRequest {
  user_uuid: string;
  role: WorkspaceRole;
}

/** Request body for PATCH /api/admin/workspaces/{id}/members/{user_uuid}. */
export interface UpdateMemberRoleRequest {
  role: WorkspaceRole;
}

/** Response shape from POST /api/admin/workspaces/{id}/members when
 *  re-adding an existing member (idempotent path). */
export interface AlreadyMemberResponse {
  workspace_id: number;
  user_uuid: string;
  status: 'already_member';
}
