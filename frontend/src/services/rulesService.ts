/**
 * Rules engine API client. The admin Settings page uses the full
 * surface; the agent toolbar only consumes `pickableActions` and
 * `apply`. All reads scope through the workspace context the cookie
 * + workspace middleware sets up; the service doesn't pass
 * workspace_id explicitly.
 *
 * Convention: list/detail queries plug into Pinia Colada `useQuery`
 * from the views; mutations call the service directly and invalidate
 * the relevant query key.
 */
import apiClient from '@nosdesk/core/apiClient';
import type {
  ApplyRuleRequest,
  ApplyRuleResponse,
  CreateRuleRequest,
  ListApplicationsQuery,
  ListRulesQuery,
  Rule,
  RuleApplication,
  RuleVersion,
  StateTransitionRequest,
  UpdateRuleRequest,
} from '@nosdesk/core/types/rule';

function toQueryString(params: object | undefined): string {
  if (!params) return '';
  const entries: string[] = [];
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined || value === null) continue;
    entries.push(`${encodeURIComponent(key)}=${encodeURIComponent(String(value))}`);
  }
  return entries.length ? `?${entries.join('&')}` : '';
}

export const rulesService = {
  /** GET /api/rules. Admin. */
  async list(query?: ListRulesQuery): Promise<Rule[]> {
    const { data } = await apiClient.get<Rule[]>(`/rules${toQueryString(query)}`);
    return data;
  },

  /** GET /api/rules/{id}. Admin. */
  async get(id: number): Promise<Rule> {
    const { data } = await apiClient.get<Rule>(`/rules/${id}`);
    return data;
  },

  /** POST /api/rules. Admin. */
  async create(req: CreateRuleRequest): Promise<Rule> {
    const { data } = await apiClient.post<Rule>('/rules', req);
    return data;
  },

  /** PUT /api/rules/{id}. Admin. */
  async update(id: number, req: UpdateRuleRequest): Promise<Rule> {
    const { data } = await apiClient.put<Rule>(`/rules/${id}`, req);
    return data;
  },

  /** PATCH /api/rules/{id}/state. Admin. */
  async transitionState(id: number, req: StateTransitionRequest): Promise<Rule> {
    const { data } = await apiClient.patch<Rule>(`/rules/${id}/state`, req);
    return data;
  },

  /** DELETE /api/rules/{id}. Admin. Soft-archive by default. */
  async archive(id: number): Promise<Rule> {
    const { data } = await apiClient.delete<Rule>(`/rules/${id}`);
    return data;
  },

  /**
   * DELETE /api/rules/{id}?hard=true. Admin. The rule must be
   * archived first; the API returns 409 RULE_NOT_ARCHIVED otherwise.
   */
  async hardDelete(id: number): Promise<void> {
    await apiClient.delete(`/rules/${id}?hard=true`);
  },

  /** GET /api/rules/{id}/versions. Admin. */
  async listVersions(ruleId: number): Promise<RuleVersion[]> {
    const { data } = await apiClient.get<RuleVersion[]>(`/rules/${ruleId}/versions`);
    return data;
  },

  /** GET /api/rules/{rule_id}/versions/{version}. Admin. */
  async getVersion(ruleId: number, version: number): Promise<RuleVersion> {
    const { data } = await apiClient.get<RuleVersion>(
      `/rules/${ruleId}/versions/${version}`,
    );
    return data;
  },

  /** GET /api/rule-applications. Admin. */
  async listApplications(query?: ListApplicationsQuery): Promise<RuleApplication[]> {
    const { data } = await apiClient.get<RuleApplication[]>(
      `/rule-applications${toQueryString(query)}`,
    );
    return data;
  },

  /** GET /api/rule-applications/{id}. Admin. */
  async getApplication(id: number): Promise<RuleApplication> {
    const { data } = await apiClient.get<RuleApplication>(`/rule-applications/${id}`);
    return data;
  },

  /** GET /api/tickets/{ticket_id}/rule-applications. Agent. */
  async listApplicationsForTicket(ticketId: number): Promise<RuleApplication[]> {
    const { data } = await apiClient.get<RuleApplication[]>(
      `/tickets/${ticketId}/rule-applications`,
    );
    return data;
  },

  /**
   * GET /api/tickets/{id}/applicable-actions. Agent. The toolbar
   * picker queries this; manual rules carry no conditions in Phase
   * 1 so the response is the unfiltered live-manual list.
   */
  async pickableActions(ticketId: number): Promise<Rule[]> {
    const { data } = await apiClient.get<Rule[]>(
      `/tickets/${ticketId}/applicable-actions`,
    );
    return data;
  },

  /** POST /api/rules/{id}/apply. Agent. */
  async apply(ruleId: number, req: ApplyRuleRequest): Promise<ApplyRuleResponse> {
    const { data } = await apiClient.post<ApplyRuleResponse>(
      `/rules/${ruleId}/apply`,
      req,
    );
    return data;
  },
};

export default rulesService;
