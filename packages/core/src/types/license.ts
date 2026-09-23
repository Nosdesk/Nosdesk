/** GET /api/admin/license: the Licence & Cloud admin page, in one read. */

export type LicenseSource = 'none' | 'env' | 'pasted' | 'linked';

/** Why a present licence grants nothing. Mirrors `license::LicenseError`. */
export type LicenseErrorKind =
  | 'malformed'
  | 'unknown_key'
  | 'bad_signature'
  | 'wrong_issuer'
  | 'invalid_claims'
  | 'expired';

export type PushMode = 'default' | 'native' | 'relay' | 'off';

export interface RelayStatus {
  /** `ok`, or a static failure kind such as `dpa_required` or `no_license`. */
  last_outcome: string | null;
  last_success_at: number | null;
  last_attempt_at: number | null;
  /** Short id of the backend process that answered; relay status is per process. */
  process_id: string;
}

export interface LicenseOverview {
  self_hosted: boolean;
  edition: 'community' | 'enterprise';
  max_workspaces: number;
  active_workspaces: number;
  can_create_workspace: boolean;
  instance_id: string;
  license: {
    source: LicenseSource;
    /** NOSDESK_LICENSE_KEY is set, so the page is read-only. */
    env_managed: boolean;
    error: LicenseErrorKind | null;
    installed_at: string | null;
    /** The daily renewal from Nosdesk Cloud is on (NOSDESK_LICENSE_AUTO_REFRESH). */
    auto_refresh: boolean;
    last_refresh_at: string | null;
    last_refresh_error: string | null;
    /** Present whenever the licence verified, including when it has expired. */
    details: {
      customer_id: string;
      licensee: string;
      license_id: string;
      max_workspaces: number;
      /** Unix seconds. */
      expires_at: number;
      features: string[];
    } | null;
  };
  push: {
    mode: PushMode;
    source: 'env' | 'stored' | 'default';
    env_managed: boolean;
    /** Live sender: `native`, `relay` or `none`. */
    sender: string;
    configured: boolean;
    native_credentials: boolean;
    relay: RelayStatus | null;
  } | null;
  /** A connection to Nosdesk Cloud this server is running or last ran. */
  link: LicenseLink | null;
}

/** Connecting this server to Nosdesk Cloud (RFC 8628 device flow). */
export interface LicenseLink {
  id: string;
  /** `XXXX-XXXX`, to type on the dashboard. */
  user_code: string;
  verification_uri: string;
  /** The dashboard page with the code filled in. */
  verification_uri_complete: string;
  /** Unix seconds. */
  expires_at: number;
  status: 'pending' | 'connected' | 'denied' | 'expired' | 'failed';
  /** Why it failed: a licence error kind, `env_managed` or `storage`. */
  error: string | null;
}
