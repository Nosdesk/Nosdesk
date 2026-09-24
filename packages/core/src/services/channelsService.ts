/**
 * Admin-side service for the multi-channel ingestion framework.
 *
 * Backs `/admin/channels/email` and (later) any UI that lists
 * non-email channels. Every call goes through the authenticated
 * `apiClient` — the endpoints require `role = admin`.
 *
 * The schema supports N channels; the phase-1 UI only surfaces a
 * single `email_imap` row and calls this service as if the list
 * always has length 0 or 1.
 */
import apiClient from '../apiClient';

/**
 * Serialized channel row. Mirrors the backend's `ChannelResponse`
 * (Channel + has_credential). Password is never returned — the flag
 * tells the UI whether to render the "rotate password" form state.
 */
export interface Channel {
  id: number;
  provider: string;
  name: string;
  enabled: boolean;
  config: Record<string, unknown>;
  runtime_state: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  last_polled_at: string | null;
  has_credential: boolean;
  /**
   * For `email_forward` channels: the generated `<token>@<inbound_domain>`
   * address the customer forwards to. Absent for other providers.
   */
  forwarding_address?: string;
  /**
   * For `email_managed` channels (hosted): the public
   * `support@<slug>.<tenant_domain>` address the workspace receives at and
   * replies from. Absent for other providers.
   */
  managed_address?: string;
}

/** Known shape of `config` for the `email_imap` provider. */
export interface ImapChannelConfig {
  host: string;
  port?: number;
  username: string;
  mailbox?: string;
  use_tls?: boolean;
  reply_domain: string;
  /**
   * Only safe for Greenmail / self-hosted test servers. The admin UI
   * surfaces this as an opt-in toggle with a prominent warning.
   */
  insecure_skip_cert_verify?: boolean;
}

/** Known shape of `runtime_state` for the `email_imap` provider. */
export interface ImapRuntimeState {
  last_seen_uid?: number;
  uid_validity?: number | null;
  last_error?: string | null;
}

export interface CreateChannelRequest {
  provider: string;
  name: string;
  enabled: boolean;
  config: Record<string, unknown>;
  /** Optional — can be set later via PATCH. */
  password?: string;
}

export interface UpdateChannelRequest {
  name?: string;
  enabled?: boolean;
  config?: Record<string, unknown>;
  /** When set, replaces the stored password. Leave undefined to keep. */
  password?: string;
}

/**
 * Result of `POST /api/admin/channels/email/test`, 200 either way. `code`
 * names the stage that failed; `detail` is the server's own words.
 */
export interface ImapTestResult {
  ok: boolean;
  code:
    | 'dns'
    | 'egress_blocked'
    | 'connect'
    | 'tls'
    | 'auth'
    | 'mailbox'
    | 'timeout'
    | 'invalid'
    | null;
  detail: string | null;
}

export const channelsService = {
  async list(): Promise<Channel[]> {
    const { data } = await apiClient.get<Channel[]>('/admin/channels');
    return data;
  },

  async get(id: number): Promise<Channel> {
    const { data } = await apiClient.get<Channel>(`/admin/channels/${id}`);
    return data;
  },

  async create(req: CreateChannelRequest): Promise<Channel> {
    const { data } = await apiClient.post<Channel>('/admin/channels', req);
    return data;
  },

  /**
   * Create a forwarding channel. The backend mints the `<token>@<domain>`
   * address (returned as `forwarding_address`) and needs no further config.
   */
  async createForwarding(name: string): Promise<Channel> {
    return this.create({
      provider: 'email_forward',
      name,
      enabled: true,
      config: {},
    });
  },

  async update(id: number, req: UpdateChannelRequest): Promise<Channel> {
    const { data } = await apiClient.patch<Channel>(`/admin/channels/${id}`, req);
    return data;
  },

  async remove(id: number): Promise<void> {
    await apiClient.delete(`/admin/channels/${id}`);
  },

  async clearCredential(id: number): Promise<void> {
    await apiClient.delete(`/admin/channels/${id}/credentials`);
  },

  /**
   * Test unsaved IMAP settings. A blank password reuses the saved channel's,
   * but only while the host and username are the saved ones.
   */
  async testImap(req: {
    channel_id?: number;
    config: Record<string, unknown>;
    password?: string;
  }): Promise<ImapTestResult> {
    const { data } = await apiClient.post<ImapTestResult>('/admin/channels/email/test', req);
    return data;
  }
};
