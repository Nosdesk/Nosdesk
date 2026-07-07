// Central User Type Definitions

import type { WorkspaceRole } from './workspace';

/** Legacy effective-role tier, kept as the vocabulary the role-picker
 * UI and dashboard widget system speak. It is no longer a wire field;
 * `effectiveRole()` derives it from the W2 split when a single tier is
 * needed for display or widget selection. */
export type UserRole = 'admin' | 'technician' | 'user' | 'audit_reviewer';

/** Platform-wide privilege role, mirrored from `users.platform_role`. */
export type PlatformRole = 'platform_admin' | 'audit_reviewer' | 'user';

/** Expand a legacy role tier (what the role-picker UI offers) back to
 * the W2 split for local state updates. Mirrors the backend's
 * `parse_roles`: a picked `admin` is a workspace admin (platform
 * `user`), not an instance super-admin. */
export function rolesFromTier(tier: UserRole): {
  platform_role: PlatformRole;
  workspace_role: WorkspaceRole;
} {
  switch (tier) {
    case 'admin':
      return { platform_role: 'user', workspace_role: 'admin' };
    case 'technician':
      return { platform_role: 'user', workspace_role: 'agent' };
    case 'audit_reviewer':
      return { platform_role: 'audit_reviewer', workspace_role: 'member' };
    default:
      return { platform_role: 'user', workspace_role: 'member' };
  }
}

/** Collapse the W2 role split (`platform_role` + the user's
 * `workspace_role`) back to the single legacy tier for display and the
 * dashboard widget system. Platform admins and workspace owners/admins
 * read as `admin`; workspace agents as `technician`; audit reviewers
 * keep their own tier; everyone else is `user`. */
export function effectiveRole(u: {
  platform_role: PlatformRole;
  workspace_role?: WorkspaceRole | null;
}): UserRole {
  if (u.platform_role === 'platform_admin') return 'admin';
  if (u.platform_role === 'audit_reviewer') return 'audit_reviewer';
  switch (u.workspace_role) {
    case 'owner':
    case 'admin':
      return 'admin';
    case 'agent':
      return 'technician';
    default:
      return 'user';
  }
}

/** Persisted shape of a user's customised dashboard. Widget order is
 * the array order; `visible: false` hides the widget without removing
 * it from the stored ordering so "Add widget" can restore it in place.
 * Missing widgets (new registry entries since the layout was saved)
 * are merged in at the tail on load — see `dashboardLayout` store.
 *
 * `span` is the column span (1, 2, or 3) the user has picked in edit
 * mode. When omitted, the client falls back to the registry default
 * for the widget.
 *
 * Placement model: list order is the vertical intent (reading order)
 * and `col` is the widget's anchor column. Rows are always DERIVED by
 * the client's gravity packer (widgets float up within their column
 * band, collisions push down), so no row is persisted. */
export interface DashboardLayout {
  widgets: Array<{
    id: string;
    visible: boolean;
    span?: 1 | 2 | 3;
    /**
     * Row span on the fixed-unit grid lattice (1-3 row units). User
     * override set by the corner-resize handle; when absent the client
     * falls back to the registry-derived default (see `rowSpanFor`).
     */
    rowSpan?: 1 | 2 | 3;
    /**
     * Anchor column (0-based) of the widget's top-left cell on the
     * 3-column desktop lattice. When absent the widget packs "auto"
     * (earliest free slot in reading order, the legacy dense-flow
     * behaviour). Written for every visible widget when an edit
     * session commits a drag or keyboard move.
     */
    col?: 0 | 1 | 2;
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
  platform_role: PlatformRole;
  workspace_role?: WorkspaceRole | null;
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
  /** Raw BCP-47 locale preference. `null` / absent means "inherit
   * from site default". The settings picker reads this; UI rendering
   * uses `effective_locale` instead. */
  locale?: string | null;
  /** Raw IANA timezone preference. `null` / absent means "use the
   * browser-detected zone". Settings picker reads this; date
   * formatting uses `effective_timezone` instead. */
  timezone?: string | null;
  /** Server-resolved locale after walking user pref -> site default
   * -> hardcoded fallback. Only present on /auth/me responses. */
  effective_locale?: string | null;
  /** Server-resolved timezone after walking the same chain. Only
   * present on /auth/me responses. */
  effective_timezone?: string | null;
  created_at: string;
  updated_at: string;
  open_ticket_count?: number;
  device_count?: number;
  /** When set, the user is soft-deleted: hidden from active
   * surfaces (mention search, assignee pickers, the default
   * admin list) but the row stays in the table for the
   * configured grace window. The retention worker hard-deletes
   * after that. Null on every active row. */
  deleted_at?: string | null;
}

/**
 * Minimal user info (for lists, dropdowns, etc.)
 */
export interface UserInfo {
  uuid: string;
  name: string;
  email: string;
  platform_role: PlatformRole;
  workspace_role?: WorkspaceRole | null;
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
