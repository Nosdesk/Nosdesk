// Microsoft Graph Integration Types

export type SyncEntityType = 'users' | 'devices' | 'groups' | 'profile_photos';

export type SyncStatus = 'starting' | 'running' | 'completed' | 'error' | 'cancelled' | 'cancelling' | 'completed_with_errors';

export interface GraphConfiguration {
  clientId: string;
  clientSecret: string;
  tenantId: string;
  scopes: string[];
}

export interface ConfigValidation {
  valid: boolean;
  message?: string;
  missing_fields?: string[];
  client_id?: string;
  tenant_id?: string;
  client_secret_configured?: boolean;
  redirect_uri?: string;
}

export interface ConnectionStatus {
  status: 'connected' | 'disconnected' | 'error';
  last_sync?: string;
  message?: string;
}

// Backend actual response structure
export interface SyncProgress {
  entity: string; // Backend uses "entity" not "entity_type"
  processed: number; // Backend uses "processed" not "current"
  total: number;
  status: string; // Backend uses string, not enum
  errors: string[]; // Backend always includes this
}

// Backend SyncProgressState structure (from get_active_syncs endpoint)
export interface ActiveSync {
  session_id: string;
  entity: string; // Backend uses "entity" not "entity_type"
  current: number; // Backend uses "current" not "processed"
  total: number;
  status: string; // Backend uses string
  message: string;
  started_at: string;
  updated_at: string;
  sync_type: string;
  is_delta: boolean;
}

// Backend SyncResult structure
export interface SyncResult {
  success: boolean;
  message: string;
  results: SyncProgress[]; // Backend returns array of SyncProgress
  total_processed: number;
  total_errors: number;
}

// Backend response for get_last_sync endpoint
export interface LastSyncDetails {
  session_id: string;
  entity: string; // Backend uses entity (singular)
  current: number;
  total: number;
  status: string;
  message: string;
  started_at: string;
  updated_at: string;
  sync_type: string;
  is_delta: boolean;
}

export interface GraphApiTestResult {
  success: boolean;
  message: string;
  api_version?: string;
  tenant_details?: Record<string, unknown>;
}

export interface PermissionTestResult {
  permission: string;
  granted: boolean;
  message?: string;
}
