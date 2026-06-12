import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type {
  Workspace,
  WorkspaceMember,
  MyWorkspaceEntry,
  CreateWorkspaceRequest,
  RenameWorkspaceRequest,
  AddMemberRequest,
  UpdateMemberRoleRequest,
} from '@/types/workspace';

/**
 * Workspaces service — backs the Phase 4 W1/W3 admin UI plus the
 * /me/workspaces switcher.
 *
 * Endpoint reference: `backend/src/handlers/admin_workspaces.rs`
 * (mounted in `backend/src/main.rs` under /api/admin/workspaces +
 * /api/me/workspaces).
 */
const workspacesService = {
  // ---------- admin lifecycle ----------------------------------------

  /** List every workspace. `includeArchived` defaults to false so the
   *  admin landing view shows only active workspaces; archived rows
   *  surface only when the operator explicitly asks. */
  async list(includeArchived = false): Promise<Workspace[]> {
    try {
      const params = includeArchived ? { include_archived: 'true' } : {};
      const response = await apiClient.get('/admin/workspaces', { params });
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list workspaces', { error });
      throw error;
    }
  },

  async create(request: CreateWorkspaceRequest): Promise<Workspace> {
    try {
      const response = await apiClient.post('/admin/workspaces', request);
      return response.data;
    } catch (error) {
      logger.error('Failed to create workspace', { error });
      throw error;
    }
  },

  async rename(id: number, request: RenameWorkspaceRequest): Promise<Workspace> {
    try {
      const response = await apiClient.patch(`/admin/workspaces/${id}`, request);
      return response.data;
    } catch (error) {
      logger.error('Failed to rename workspace', { error, id });
      throw error;
    }
  },

  async archive(id: number): Promise<Workspace> {
    try {
      const response = await apiClient.post(`/admin/workspaces/${id}/archive`);
      return response.data;
    } catch (error) {
      logger.error('Failed to archive workspace', { error, id });
      throw error;
    }
  },

  async restore(id: number): Promise<Workspace> {
    try {
      const response = await apiClient.post(`/admin/workspaces/${id}/restore`);
      return response.data;
    } catch (error) {
      logger.error('Failed to restore workspace', { error, id });
      throw error;
    }
  },

  /**
   * Hard-delete requires `?confirm=<slug>` matching the workspace's
   * slug exactly. Backend returns 409 if the row isn't archived or
   * the confirm doesn't match.
   */
  async hardDelete(id: number, confirmSlug: string): Promise<void> {
    try {
      await apiClient.delete(`/admin/workspaces/${id}`, {
        params: { confirm: confirmSlug },
      });
    } catch (error) {
      logger.error('Failed to hard-delete workspace', { error, id });
      throw error;
    }
  },

  // ---------- admin membership management ----------------------------

  async listMembers(workspaceId: number): Promise<WorkspaceMember[]> {
    try {
      const response = await apiClient.get(
        `/admin/workspaces/${workspaceId}/members`,
      );
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list workspace members', { error, workspaceId });
      throw error;
    }
  },

  /**
   * Add a user to a workspace. Idempotent: re-adding an existing
   * member returns 200 with `{ status: 'already_member' }` rather
   * than 409. Callers can detect this via the returned shape if
   * they care to differentiate.
   */
  async addMember(
    workspaceId: number,
    request: AddMemberRequest,
  ): Promise<WorkspaceMember | { workspace_id: number; user_uuid: string; status: 'already_member' }> {
    try {
      const response = await apiClient.post(
        `/admin/workspaces/${workspaceId}/members`,
        request,
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to add workspace member', { error, workspaceId });
      throw error;
    }
  },

  async updateMemberRole(
    workspaceId: number,
    userUuid: string,
    request: UpdateMemberRoleRequest,
  ): Promise<WorkspaceMember> {
    try {
      const response = await apiClient.patch(
        `/admin/workspaces/${workspaceId}/members/${userUuid}`,
        request,
      );
      return response.data;
    } catch (error) {
      logger.error('Failed to update workspace member role', { error, workspaceId, userUuid });
      throw error;
    }
  },

  async removeMember(workspaceId: number, userUuid: string): Promise<void> {
    try {
      await apiClient.delete(
        `/admin/workspaces/${workspaceId}/members/${userUuid}`,
      );
    } catch (error) {
      logger.error('Failed to remove workspace member', { error, workspaceId, userUuid });
      throw error;
    }
  },

  // ---------- tenant self-serve membership (caller's own workspace) --
  //
  // These act on the request's workspace context (no id in the path),
  // gated on workspace-admin. Distinct from the platform-admin operator
  // methods above, which target an arbitrary workspace by id.

  async listWorkspaceMembers(): Promise<WorkspaceMember[]> {
    try {
      const response = await apiClient.get('/workspace/members');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to list workspace members', { error });
      throw error;
    }
  },

  async updateWorkspaceMemberRole(
    userUuid: string,
    request: UpdateMemberRoleRequest,
  ): Promise<WorkspaceMember> {
    try {
      const response = await apiClient.patch(`/workspace/members/${userUuid}`, request);
      return response.data;
    } catch (error) {
      logger.error('Failed to update workspace member role', { error, userUuid });
      throw error;
    }
  },

  async removeWorkspaceMember(userUuid: string): Promise<void> {
    try {
      await apiClient.delete(`/workspace/members/${userUuid}`);
    } catch (error) {
      logger.error('Failed to remove workspace member', { error, userUuid });
      throw error;
    }
  },

  // ---------- /api/me/workspaces (caller's own memberships) ----------

  async listMyWorkspaces(): Promise<MyWorkspaceEntry[]> {
    try {
      const response = await apiClient.get('/me/workspaces');
      return response.data || [];
    } catch (error) {
      logger.error('Failed to load own workspaces', { error });
      throw error;
    }
  },
};

export default workspacesService;
