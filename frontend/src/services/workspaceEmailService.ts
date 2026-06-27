import apiClient from '@nosdesk/core/apiClient';

export interface DkimRecord {
  name: string;
  txt_value: string;
}

export interface OutboundSettings {
  sending_mode: string;
  from_name: string;
  from_email: string;
  sending_domain: string | null;
  verification_status: string;
  verified_at: string | null;
  dkim_record: DkimRecord | null;
}

export interface SetDomainResponse {
  dkim_record: DkimRecord;
  verification_status: string;
}

export type CheckStatus = 'pass' | 'warn' | 'fail' | 'info';

export interface RecordCheck {
  status: CheckStatus;
  summary: string;
  value: string | null;
}

export interface EmailAuthReport {
  domain: string;
  spf: RecordCheck;
  dkim: RecordCheck;
  dmarc: RecordCheck;
  mx: RecordCheck;
}

/// Admin API for a workspace's verified sending domain (DKIM via the instance
/// relay). Mirrors the `/admin/email/outbound` endpoints.
export default {
  async get(): Promise<OutboundSettings> {
    const response = await apiClient.get<OutboundSettings>('/admin/email/outbound');
    return response.data;
  },

  async setDomain(payload: { from_name: string; from_email: string }): Promise<SetDomainResponse> {
    const response = await apiClient.put<SetDomainResponse>(
      '/admin/email/outbound/domain',
      payload,
    );
    return response.data;
  },

  async verify(): Promise<{ verification_status: string }> {
    const response = await apiClient.post<{ verification_status: string }>(
      '/admin/email/outbound/verify',
      {},
    );
    return response.data;
  },

  async dnsCheck(): Promise<EmailAuthReport> {
    const response = await apiClient.get<EmailAuthReport>('/admin/email/outbound/dns-check');
    return response.data;
  },

  async sendTest(): Promise<{ status: string; to: string }> {
    const response = await apiClient.post<{ status: string; to: string }>(
      '/admin/email/outbound/test',
      {},
    );
    return response.data;
  },

  async reset(): Promise<void> {
    await apiClient.delete('/admin/email/outbound');
  },
};
