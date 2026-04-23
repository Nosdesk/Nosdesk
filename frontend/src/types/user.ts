// Central User Type Definitions

export type UserRole = 'admin' | 'technician' | 'user';

/** Persisted shape of a user's customised dashboard. Widget order is
 * the array order; `visible: false` hides the widget without removing
 * it from the stored ordering so "Add widget" can restore it in place.
 * Missing widgets (new registry entries since the layout was saved)
 * are merged in at the tail on load — see `dashboardLayout` store.
 *
 * `span` is the column span (1, 2, or 3) the user has picked in edit
 * mode. When omitted, the client falls back to the registry default
 * for the widget. Extending this to `{ x, y, w, h }` later is a
 * non-breaking superset — existing entries keep working. */
export interface DashboardLayout {
  widgets: Array<{
    id: string;
    visible: boolean;
    span?: 1 | 2 | 3;
    /**
     * Per-widget configuration bag. Shape is owned by each widget —
     * the layout system treats it as opaque JSON. Used for widgets
     * that expose user-facing settings (the Queue KPI picker, default
     * filter for the ticket list, etc.).
     */
    config?: Record<string, unknown>;
  }>;
}

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
  /** Email signature appended to outbound channel replies. */
  signature?: string | null;
  /** Per-user customised dashboard layout. `null` / absent means the
   * client falls back to the role default. */
  dashboard_layout?: DashboardLayout | null;
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
  /** Empty string clears the stored signature; omit to leave unchanged. */
  signature?: string | null;
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
