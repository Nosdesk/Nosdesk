/**
 * API Token Types
 * For programmatic API access management
 */

export interface ApiToken {
  uuid: string;
  token_prefix: string;
  name: string;
  user_uuid: string;
  user_name: string;
  scopes: string[];
  created_at: string;
  created_by_name: string;
  expires_at: string | null;
  revoked_at: string | null;
  last_used_at: string | null;
}

export interface ApiTokenCreated {
  uuid: string;
  token: string; // Raw token - only shown once!
  token_prefix: string;
  name: string;
  user_uuid: string;
  expires_at: string | null;
}

export interface CreateApiTokenRequest {
  name: string;
  user_uuid: string;
  expires_in_days?: number | null;
  scopes?: string[];
}
