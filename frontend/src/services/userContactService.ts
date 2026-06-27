import apiClient from '@nosdesk/core/apiClient';
import { logger } from '@nosdesk/core/utils/logger';

// ---- Types -----------------------------------------------------------------

/** The workspace's user custom-field schema (JSON-Schema subset object). */
export type UserFieldSchema = Record<string, unknown>;

export interface UserProfileFields {
  user_uuid: string;
  job_title: string | null;
  organization: string | null;
  department: string | null;
  custom_fields: Record<string, unknown>;
  /** True when the standard cols came from the directory sync (read-only). */
  directory_synced: boolean;
}

export interface UserPhone {
  id: number;
  user_uuid: string;
  phone: string;
  phone_type: 'work' | 'mobile' | 'other';
  is_primary: boolean;
  /** 'microsoft' = synced/read-only; null = manual. */
  source: string | null;
  label: string | null;
}

export interface UserPhoneInput {
  phone: string;
  phone_type: string;
  is_primary?: boolean;
  label?: string | null;
}

export interface UserAddress {
  id: number;
  user_uuid: string;
  address_type: 'work' | 'home' | 'other';
  is_primary: boolean;
  street: string | null;
  city: string | null;
  region: string | null;
  postal_code: string | null;
  country: string | null;
  source: string | null;
  label: string | null;
}

export interface UserAddressInput {
  address_type: string;
  is_primary?: boolean;
  street?: string | null;
  city?: string | null;
  region?: string | null;
  postal_code?: string | null;
  country?: string | null;
  label?: string | null;
}

// ---- Workspace user-field schema (admin) -----------------------------------

export const USER_FIELD_SCHEMA_QUERY_KEY = ['user-field-schema'] as const;

export const getUserFieldSchema = async (): Promise<UserFieldSchema> => {
  const response = await apiClient.get('/admin/user-fields');
  return response.data as UserFieldSchema;
};

export const setUserFieldSchema = async (
  schema: UserFieldSchema,
  force = false,
): Promise<UserFieldSchema> => {
  const response = await apiClient.put('/admin/user-fields', schema, {
    params: force ? { force: 'true' } : undefined,
  });
  return response.data as UserFieldSchema;
};

// ---- Per-user profile fields ------------------------------------------------

export const getUserProfileFields = async (uuid: string): Promise<UserProfileFields> => {
  const response = await apiClient.get(`/users/${uuid}/profile-fields`);
  return response.data as UserProfileFields;
};

export const setUserProfileFields = async (
  uuid: string,
  body: {
    job_title?: string | null;
    organization?: string | null;
    department?: string | null;
    custom_fields?: Record<string, unknown>;
  },
): Promise<UserProfileFields> => {
  try {
    const response = await apiClient.put(`/users/${uuid}/profile-fields`, body);
    return response.data as UserProfileFields;
  } catch (error) {
    logger.error('Failed to save profile fields', { error, uuid });
    throw error;
  }
};

// ---- Phones -----------------------------------------------------------------

export const listUserPhones = async (uuid: string): Promise<UserPhone[]> => {
  const response = await apiClient.get(`/users/${uuid}/phones`);
  return response.data as UserPhone[];
};

export const addUserPhone = async (uuid: string, body: UserPhoneInput): Promise<UserPhone> => {
  const response = await apiClient.post(`/users/${uuid}/phones`, body);
  return response.data as UserPhone;
};

export const updateUserPhone = async (
  uuid: string,
  id: number,
  body: UserPhoneInput,
): Promise<UserPhone> => {
  const response = await apiClient.put(`/users/${uuid}/phones/${id}`, body);
  return response.data as UserPhone;
};

export const deleteUserPhone = async (uuid: string, id: number): Promise<void> => {
  await apiClient.delete(`/users/${uuid}/phones/${id}`);
};

// ---- Addresses --------------------------------------------------------------

export const listUserAddresses = async (uuid: string): Promise<UserAddress[]> => {
  const response = await apiClient.get(`/users/${uuid}/addresses`);
  return response.data as UserAddress[];
};

export const addUserAddress = async (uuid: string, body: UserAddressInput): Promise<UserAddress> => {
  const response = await apiClient.post(`/users/${uuid}/addresses`, body);
  return response.data as UserAddress;
};

export const updateUserAddress = async (
  uuid: string,
  id: number,
  body: UserAddressInput,
): Promise<UserAddress> => {
  const response = await apiClient.put(`/users/${uuid}/addresses/${id}`, body);
  return response.data as UserAddress;
};

export const deleteUserAddress = async (uuid: string, id: number): Promise<void> => {
  await apiClient.delete(`/users/${uuid}/addresses/${id}`);
};
