/**
 * Reusable reply templates that techs can insert into the ticket
 * composer with one click. Reads open to any authenticated user so
 * the composer picker works for all techs; writes (CRUD) go through
 * the admin endpoints.
 *
 * Template variable substitution ({{ticket_id}}, {{customer_name}},
 * etc.) happens client-side at insert time — see `renderTemplate`.
 * That keeps the backend free of ticket-specific lookups for the
 * common case, and lets the tech see the final text before sending.
 */
import apiClient from './apiConfig';

export interface CannedResponse {
  id: number;
  title: string;
  body: string;
  created_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateCannedResponseRequest {
  title: string;
  body: string;
}

export interface UpdateCannedResponseRequest {
  title?: string;
  body?: string;
}

export const cannedResponsesService = {
  async list(): Promise<CannedResponse[]> {
    const { data } = await apiClient.get<CannedResponse[]>('/canned-responses');
    return data;
  },
  async create(req: CreateCannedResponseRequest): Promise<CannedResponse> {
    const { data } = await apiClient.post<CannedResponse>(
      '/admin/canned-responses',
      req,
    );
    return data;
  },
  async update(id: number, req: UpdateCannedResponseRequest): Promise<CannedResponse> {
    const { data } = await apiClient.patch<CannedResponse>(
      `/admin/canned-responses/${id}`,
      req,
    );
    return data;
  },
  async remove(id: number): Promise<void> {
    await apiClient.delete(`/admin/canned-responses/${id}`);
  },
};

/**
 * Values the template engine knows how to substitute. Keep this
 * lean — every new variable means a bigger context the composer has
 * to assemble, which is friction the tech will feel.
 */
export interface TemplateVars {
  ticket_id?: number | string;
  ticket_title?: string;
  customer_name?: string;
  tech_name?: string;
  app_name?: string;
}

/**
 * Plain `{{variable}}` substitution — no Handlebars, no HTML
 * escaping (the composer is plain-text / markdown). Unknown tokens
 * are left intact so a tech editing the template spots their own
 * typos in the result they're about to send.
 */
export function renderTemplate(template: string, vars: TemplateVars): string {
  const lookup: Record<string, string> = {
    ticket_id: vars.ticket_id != null ? String(vars.ticket_id) : '',
    ticket_title: vars.ticket_title ?? '',
    customer_name: vars.customer_name ?? '',
    tech_name: vars.tech_name ?? '',
    app_name: vars.app_name ?? '',
  };
  return template.replace(/\{\{(\w+)\}\}/g, (match, key) => {
    // `match` is the whole `{{key}}` token; fall back to leaving it
    // in place when the key isn't recognised (not `""`).
    return Object.prototype.hasOwnProperty.call(lookup, key) ? lookup[key] : match;
  });
}
