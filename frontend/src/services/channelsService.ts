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
import apiClient from '@nosdesk/core/apiClient';

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
 * Result of `POST /api/admin/channels/{id}/test-connection`. The
 * backend always responds 200; the `ok` field distinguishes a
 * successful probe from a reachable server that rejected login
 * (wrong credentials, missing mailbox, etc.) so the UI can render
 * the operator-facing error message verbatim.
 */
export interface TestConnectionResult {
  ok: boolean;
  error?: string;
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
   * Probe the channel's IMAP server. Either uses the stored password
   * (when `password` is omitted) or a candidate the admin typed into
   * the form before saving.
   */
  async testConnection(id: number, password?: string): Promise<TestConnectionResult> {
    const { data } = await apiClient.post<TestConnectionResult>(
      `/admin/channels/${id}/test-connection`,
      password ? { password } : {}
    );
    return data;
  }
};
