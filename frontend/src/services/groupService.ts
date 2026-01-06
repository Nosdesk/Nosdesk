import apiClient from './apiConfig';
import { logger } from '@/utils/logger';
import type {
  Group,
  GroupWithMemberCount,
  GroupWithMembers,
  GroupDetails,
  CreateGroupRequest,
  UpdateGroupRequest,
  SetGroupMembersRequest,
  SetUserGroupsRequest,
  SetGroupDevicesRequest
} from '@/types/group';

export const groupService = {
  // Get all groups with member counts (admin only)
  async getGroups(): Promise<GroupWithMemberCount[]> {
    try {
      const response = await apiClient.get<GroupWithMemberCount[]>('/groups');
      return response.data;
    } catch (error) {
      logger.error('Error fetching groups:', error);
      throw error;
    }
  },

  // Get a single group with members (admin only)
  async getGroup(id: number): Promise<GroupWithMembers> {
    try {
      const response = await apiClient.get<GroupWithMembers>(`/groups/${id}`);
      return response.data;
    } catch (error) {
      logger.error(`Error fetching group ${id}:`, error);
      throw error;
    }
  },

  // Create a new group (admin only)
  async createGroup(request: CreateGroupRequest): Promise<Group> {
    try {
      const response = await apiClient.post<Group>('/groups', request);
      return response.data;
    } catch (error) {
      logger.error('Error creating group:', error);
      throw error;
    }
  },

  // Update a group (admin only)
  async updateGroup(id: number, request: UpdateGroupRequest): Promise<Group> {
    try {
      const response = await apiClient.put<Group>(`/groups/${id}`, request);
      return response.data;
    } catch (error) {
      logger.error(`Error updating group ${id}:`, error);
      throw error;
    }
  },

  // Delete a group (admin only)
  async deleteGroup(id: number): Promise<void> {
    try {
      await apiClient.delete(`/groups/${id}`);
    } catch (error) {
      logger.error(`Error deleting group ${id}:`, error);
      throw error;
    }
  },

  // Set group members (admin only)
  async setGroupMembers(id: number, request: SetGroupMembersRequest): Promise<GroupWithMembers> {
    try {
      const response = await apiClient.put<GroupWithMembers>(`/groups/${id}/members`, request);
      return response.data;
    } catch (error) {
      logger.error(`Error setting members for group ${id}:`, error);
      throw error;
    }
  },

  // Set group devices (admin only)
  async setGroupDevices(id: number, request: SetGroupDevicesRequest): Promise<GroupDetails> {
    try {
      const response = await apiClient.put<GroupDetails>(`/groups/${id}/devices`, request);
      return response.data;
    } catch (error) {
      logger.error(`Error setting devices for group ${id}:`, error);
      throw error;
    }
  },

  // Get groups for a user (admin only)
  async getUserGroups(userUuid: string): Promise<Group[]> {
    try {
      const response = await apiClient.get<Group[]>(`/users/${userUuid}/groups`);
      return response.data;
    } catch (error) {
      logger.error(`Error fetching groups for user ${userUuid}:`, error);
      throw error;
    }
  },

  // Set groups for a user (admin only)
  async setUserGroups(userUuid: string, request: SetUserGroupsRequest): Promise<Group[]> {
    try {
      const response = await apiClient.put<Group[]>(`/users/${userUuid}/groups`, request);
      return response.data;
    } catch (error) {
      logger.error(`Error setting groups for user ${userUuid}:`, error);
      throw error;
    }
  },

  // Get group details by UUID (all authenticated users)
  async getGroupDetails(uuid: string): Promise<GroupDetails> {
    try {
      const response = await apiClient.get<GroupDetails>(`/groups/details/${uuid}`);
      return response.data;
    } catch (error) {
      logger.error(`Error fetching group details for ${uuid}:`, error);
      throw error;
    }
  },

  // Unmanage a group (remove external sync) - admin only
  async unmanageGroup(id: number): Promise<Group> {
    try {
      const response = await apiClient.post<Group>(`/groups/${id}/unmanage`);
      return response.data;
    } catch (error) {
      logger.error(`Error unmanaging group ${id}:`, error);
      throw error;
    }
  }
};

export default groupService;
