// Microsoft Graph Integration Types

export type SyncEntityType = 'users' | 'devices' | 'groups' | 'profile_photos';

export type SyncStatus = 'starting' | 'running' | 'completed' | 'error' | 'cancelled' | 'completed_with_errors';

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

export interface SyncProgress {
  session_id: string;
  status: SyncStatus;
  entity_type?: SyncEntityType;
  total?: number;
  processed?: number;
  created?: number;
  updated?: number;
  failed?: number;
  errors?: string[];
  started_at?: string;
  completed_at?: string;
  message?: string;
}

export interface ActiveSync {
  session_id: string;
  entity_type: SyncEntityType;
  status: SyncStatus;
  started_at: string;
  progress?: {
    total: number;
    processed: number;
  };
}

export interface SyncResult {
  success: boolean;
  message: string;
  session_id?: string;
  results?: {
    [key in SyncEntityType]?: {
      total: number;
      created: number;
      updated: number;
      failed: number;
      errors?: string[];
    };
  };
}

export interface LastSyncDetails {
  session_id: string;
  entities: SyncEntityType[];
  status: SyncStatus;
  started_at: string;
  completed_at?: string;
  duration?: number;
  results?: {
    [key in SyncEntityType]?: {
      total: number;
      created: number;
      updated: number;
      failed: number;
    };
  };
  errors?: string[];
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
