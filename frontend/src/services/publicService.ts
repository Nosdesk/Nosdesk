import axios from 'axios';
import { apiBaseUrl } from '@nosdesk/core/transport';
import type { WorkflowStateCategory } from '@nosdesk/core/types/workflow';

export interface PublicSiteSettings {
  app_name: string;
  logo_url: string | null;
  logo_light_url: string | null;
  favicon_url: string | null;
  primary_color: string | null;
  guest_tickets_enabled: boolean;
  guest_public_docs_enabled: boolean;
  guest_kb_search_enabled: boolean;
  guest_ticket_lookup_enabled: boolean;
  guest_help_page_enabled: boolean;
  guest_ticket_attachments_enabled: boolean;
  /** Admin-configured plain-text blurb shown above the submit form. */
  guest_ticket_intro_message: string | null;
}

export interface SubmitGuestTicketRequest {
  name: string;
  email: string;
  title: string;
  description: string;
  priority?: 'low' | 'medium' | 'high';
  /** Attachment IDs returned from POST /api/public/files/temp. Max 5. */
  attachment_ids?: number[];
  /**
   * Honeypot field. Always sent as an empty string by the real form.
   * If anything non-empty arrives, the server rejects it — naive bots
   * auto-fill every input they find.
   */
  website?: string;
}

/** Response from POST /api/public/files/temp. Id is echoed on submission. */
export interface GuestAttachmentUpload {
  id: number;
  name: string;
  size: number;
  mime_type: string;
}

/**
 * Submission response. When `verification_required` is true the ticket is
 * held in a pending-verification state and the submitter must click the
 * confirmation email link before the ticket is surfaced to techs — no
 * ticket id or lookup token is disclosed until then.
 */
export interface SubmitGuestTicketResponse {
  verification_required: boolean;
  email_sent: boolean;
  ticket_id?: number;
  lookup_token?: string;
  status_url?: string;
}

export interface GuestTicketStatus {
  ticket_id: number;
  title: string;
  category: WorkflowStateCategory;
  priority: string;
  created_at: string;
  updated_at: string;
  closed_at: string | null;
}

export interface PublicDocSummary {
  id: number;
  uuid: string;
  title: string;
  slug: string;
  icon: string | null;
  updated_at: string;
}

export interface PublicDoc extends PublicDocSummary {
  yjs_document: number[] | null;
}

// Dedicated axios instance with no credentials — we do not want the session
// cookie leaking into public endpoints, and we do not want 401 refresh
// interceptors running on anonymous flows. The base URL is resolved from the
// transport seam per-request (the seam is configured at bootstrap, after this
// module loads); no auth strategy is applied here by design.
const publicApi = axios.create({
  withCredentials: false,
  headers: { 'Content-Type': 'application/json' }
});

publicApi.interceptors.request.use((config) => {
  config.baseURL = `${apiBaseUrl()}/public`;
  return config;
});

export const publicService = {
  async getSettings(): Promise<PublicSiteSettings> {
    const { data } = await publicApi.get<PublicSiteSettings>('/settings');
    return data;
  },

  async submitTicket(body: SubmitGuestTicketRequest): Promise<SubmitGuestTicketResponse> {
    const { data } = await publicApi.post<SubmitGuestTicketResponse>('/tickets', body);
    return data;
  },

  async getTicketStatus(token: string): Promise<GuestTicketStatus> {
    const { data } = await publicApi.get<GuestTicketStatus>(`/tickets/${encodeURIComponent(token)}`);
    return data;
  },

  async listDocs(): Promise<PublicDocSummary[]> {
    const { data } = await publicApi.get<PublicDocSummary[]>('/docs');
    return data;
  },

  async getDoc(slug: string): Promise<PublicDoc> {
    const { data } = await publicApi.get<PublicDoc>(`/docs/${encodeURIComponent(slug)}`);
    return data;
  },

  async searchDocs(q: string): Promise<PublicDocSummary[]> {
    const { data } = await publicApi.get<PublicDocSummary[]>('/docs/search', { params: { q } });
    return data;
  },

  async uploadAttachment(file: File): Promise<GuestAttachmentUpload> {
    const form = new FormData();
    form.append('file', file);
    const { data } = await publicApi.post<GuestAttachmentUpload>('/files/temp', form, {
      // Let the browser set the multipart boundary; overriding here breaks it.
      headers: { 'Content-Type': 'multipart/form-data' }
    });
    return data;
  }
};

// ---- Admin-side guest settings service (uses authenticated client) ----
import apiClient from './apiConfig';

export interface AdminGuestSettings {
  guest_tickets_enabled: boolean;
  guest_public_docs_enabled: boolean;
  guest_kb_search_enabled: boolean;
  guest_ticket_lookup_enabled: boolean;
  guest_help_page_enabled: boolean;
  guest_ticket_default_priority: string | null;
  guest_ticket_rate_limit_per_hour: number;
  guest_ticket_email_verification: boolean;
  guest_ticket_attachments_enabled: boolean;
  guest_ticket_intro_message: string | null;
}

export interface AdminGuestSettingsUpdate {
  guest_tickets_enabled?: boolean;
  guest_public_docs_enabled?: boolean;
  guest_kb_search_enabled?: boolean;
  guest_ticket_lookup_enabled?: boolean;
  guest_help_page_enabled?: boolean;
  guest_ticket_default_priority?: string | null;
  guest_ticket_rate_limit_per_hour?: number;
  guest_ticket_email_verification?: boolean;
  guest_ticket_attachments_enabled?: boolean;
  guest_ticket_intro_message?: string | null;
}

export const adminGuestSettingsService = {
  async get(): Promise<AdminGuestSettings> {
    const { data } = await apiClient.get<AdminGuestSettings>('/admin/guest-settings');
    return data;
  },
  async update(update: AdminGuestSettingsUpdate): Promise<AdminGuestSettings> {
    const { data } = await apiClient.patch<AdminGuestSettings>('/admin/guest-settings', update);
    return data;
  }
};
