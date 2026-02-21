// Central User Type Definitions

export type UserRole = 'admin' | 'technician' | 'user';

/**
 * Complete user object with all fields
 */
export interface User {
  uuid: string;
  name: string;
  email: string;
  role: UserRole;
  pronouns?: string | null;
  avatar_url?: string | null;
  banner_url?: string | null;
  avatar_thumb?: string | null;
  theme?: string | null;
  created_at: string;
  updated_at: string;
  open_ticket_count?: number;
  device_count?: number;
}

/**
 * Minimal user info (for lists, dropdowns, etc.)
 */
export interface UserInfo {
  uuid: string;
  name: string;
  email: string;
  role: UserRole;
  avatar_url?: string | null;
  avatar_thumb?: string | null;
}

/**
 * User profile update payload
 */
export interface UserProfileUpdate {
  name?: string;
  email?: string;
  pronouns?: string | null;
  avatar_url?: string | null;
  banner_url?: string | null;
}

/**
 * User creation payload (admin)
 */
export interface CreateUserPayload {
  name: string;
  email: string;
  role: UserRole;
  password?: string;
}

/**
 * Login credentials
 */
export interface LoginCredentials {
  email: string;
  password: string;
}

/**
 * Security info for a user (admin viewing another user's security settings)
 */
export interface UserSecurityInfo {
  mfa_enabled: boolean;
  has_backup_codes: boolean;
  passkey_count: number;
  passkeys: {
    id: string;
    name: string;
    created_at: string;
    last_used_at: string | null;
    transports: string[];
    backup_eligible: boolean;
  }[];
  auth_identities: {
    id: number;
    provider_type: string;
    provider_name: string;
    email: string | null;
    created_at: string;
  }[];
}

/**
 * User session information
 */
export interface UserSession {
  session_token: string; // Session identifier
  user_uuid: string;
  device_name?: string;
  ip_address?: string;
  user_agent?: string;
  location?: string;
  created_at: string;
  expires_at: string;
  is_current: boolean;
}
