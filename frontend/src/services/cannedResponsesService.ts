/**
 * Reusable reply templates that techs can insert into the ticket
 * composer with one click. Reads open to any authenticated user so
 * the composer picker works for all techs; writes (CRUD) go through
 * the admin endpoints.
 *
 * Template variable substitution ({{ticket_id}}, {{customer_name}},
 * etc.) happens client-side at insert time, see `renderTemplate`.
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

/**
 * Shape returned by the list endpoint. Adds the workspace_id (the
 * picker doesn't read it; the admin page may) and the rolling 30-day
 * insertion count surfaced as the "Inserts (30d)" column.
 */
export interface CannedResponseListItem extends CannedResponse {
  workspace_id: number;
  inserts_30d: number;
}

/**
 * Starter template returned by the admin catalog endpoint. Not a
 * persisted entity, the admin picks one and the editor pre-fills.
 * Saving creates a fresh canned response row like any other.
 */
export interface CannedResponseStarter {
  slug: string;
  title: string;
  body: string;
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
  async list(): Promise<CannedResponseListItem[]> {
    const { data } = await apiClient.get<CannedResponseListItem[]>('/canned-responses');
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
  /**
   * Fire-and-forget. Backend swallows FK violations (template
   * deleted between fetch and insert) and returns 200, so the
   * picker keeps working even if the log row can't land.
   */
  async recordInsertion(id: number, ticketId?: number): Promise<void> {
    try {
      await apiClient.post(`/canned-responses/${id}/insertions`, {
        ticket_id: ticketId ?? null,
      });
    } catch {
      // Insertion logging never blocks the user-facing flow; the
      // backend already treats every failure path as 200 so this
      // catch is for transport-level errors (offline, etc.).
    }
  },
  async getStarterCatalog(): Promise<CannedResponseStarter[]> {
    // Sits at its own path on the backend (not under
    // /admin/canned-responses/) so the {id} sibling can't shadow it.
    const { data } = await apiClient.get<CannedResponseStarter[]>(
      '/admin/canned-response-starters',
    );
    return data;
  },
};

/**
 * Values the template engine knows how to substitute. Keep this
 * lean, every new variable means a bigger context the composer has
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
 * The variable allow-list mirrored on the frontend. Keep in sync
 * with `CANNED_RESPONSE_VARIABLES` in `backend/src/utils/template_variables.rs`.
 * The admin editor's `{{` autocomplete reads this; the unknown-
 * variable validator below uses it; the picker's compose-time
 * warning surfaces a hint when a body references one of these
 * with no value bound in the current ticket context.
 */
export const CANNED_RESPONSE_VARIABLES = [
  'ticket_id',
  'ticket_title',
  'customer_name',
  'customer_first_name',
  'tech_name',
  'tech_first_name',
  'app_name',
] as const;

export type CannedResponseVariable = (typeof CANNED_RESPONSE_VARIABLES)[number];

/**
 * Take the first whitespace-separated token of a full name.
 * "Mary Jane Smith" → "Mary"; empty input returns empty. The
 * picker derives `*_first_name` variants from the matching full-
 * name field so admins don't pass twice.
 */
function firstWord(value: string | undefined): string {
  return (value ?? '').trim().split(/\s+/)[0] ?? '';
}

/**
 * Plain `{{variable}}` substitution, no Handlebars, no HTML
 * escaping (the composer is plain-text / markdown). Unknown tokens
 * are left intact so a tech editing the template spots their own
 * typos in the result they're about to send.
 *
 * `customer_first_name` and `tech_first_name` are derived from
 * the matching `*_name` field rather than separately passed; the
 * caller only needs to supply the full names once.
 */
export function renderTemplate(template: string, vars: TemplateVars): string {
  const lookup: Record<string, string> = {
    ticket_id: vars.ticket_id != null ? String(vars.ticket_id) : '',
    ticket_title: vars.ticket_title ?? '',
    customer_name: vars.customer_name ?? '',
    customer_first_name: firstWord(vars.customer_name),
    tech_name: vars.tech_name ?? '',
    tech_first_name: firstWord(vars.tech_name),
    app_name: vars.app_name ?? '',
  };
  // Whitespace-tolerant token match so `{{ ticket_id }}` and
  // `{{ticket_id}}` substitute identically. Matches the backend
  // substituter at backend/src/utils/template_variables.rs which
  // the rules engine uses on apply; without this, a body authored
  // with padded braces would preview unsubstituted on the agent
  // dialog but render correctly when the rule is applied.
  return template.replace(/\{\{\s*(\w+)\s*\}\}/g, (match, key) => {
    // `match` is the whole `{{key}}` token; fall back to leaving it
    // in place when the key isn't recognised (not `""`).
    return Object.prototype.hasOwnProperty.call(lookup, key) ? lookup[key] : match;
  });
}

/**
 * Sorted, deduped list of `{{token}}` names in `body` that aren't
 * on the allow-list. Empty when every token is recognised. The
 * admin editor surfaces a non-empty return as an inline error so
 * the save round-trip can't introduce a typo'd template.
 */
export function findUnknownVariables(body: string): string[] {
  const allowed: Set<string> = new Set<string>(CANNED_RESPONSE_VARIABLES);
  const seen: Set<string> = new Set<string>();
  const re = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(body)) !== null) {
    const name = match[1];
    if (!allowed.has(name)) {
      seen.add(name);
    }
  }
  return Array.from(seen).sort();
}

/**
 * The set of allow-listed `{{tokens}}` actually referenced by
 * `body`. Used by the picker's compose-time warning: if the
 * rendered output would substitute `customer_name` with an empty
 * string because the current ticket has no customer, the picker
 * surfaces a one-line hint above the composer.
 */
export function variablesUsed(body: string): CannedResponseVariable[] {
  const allowed: Set<string> = new Set<string>(CANNED_RESPONSE_VARIABLES);
  const seen: Set<string> = new Set<string>();
  const re = /\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}/g;
  let match: RegExpExecArray | null;
  while ((match = re.exec(body)) !== null) {
    const name = match[1];
    if (allowed.has(name)) {
      seen.add(name);
    }
  }
  return Array.from(seen).sort() as CannedResponseVariable[];
}

/**
 * Allow-listed variables `body` references that would substitute
 * to empty against the current ticket context. The check resolves
 * each name through `renderTemplate` so derived variables (e.g.
 * `customer_first_name` is empty iff `customer_name` is empty)
 * are handled correctly without the caller knowing about the
 * derivation rules.
 */
export function unboundVariables(
  body: string,
  vars: TemplateVars,
): CannedResponseVariable[] {
  return variablesUsed(body).filter((name) => {
    const resolved = renderTemplate(`{{${name}}}`, vars);
    return resolved.trim() === '';
  });
}

export default cannedResponsesService;
