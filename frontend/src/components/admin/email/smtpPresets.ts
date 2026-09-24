import type { SmtpSecurity } from '@nosdesk/core/services/workspaceEmailService';

/**
 * Quick fills for the own-server form. A preset only fills host, port and
 * security and shows one line of help; everything stays editable. SES has no
 * host because the endpoint depends on the account's region.
 */
export interface SmtpPreset {
  id: 'm365' | 'google' | 'ses' | 'mailgun' | 'postmark' | 'other';
  /** Provider name, not translated. */
  name: string;
  host?: string;
  hostPlaceholder?: string;
  port?: number;
  security?: SmtpSecurity;
  /** Fluent key for the provider's one line of help. */
  helpKey?: string;
}

export const SMTP_PRESETS: SmtpPreset[] = [
  {
    id: 'm365',
    name: 'Microsoft 365',
    host: 'smtp.office365.com',
    port: 587,
    security: 'starttls',
    helpKey: 'email-relay-preset-m365-help',
  },
  {
    id: 'google',
    name: 'Google Workspace',
    host: 'smtp.gmail.com',
    port: 587,
    security: 'starttls',
    helpKey: 'email-relay-preset-google-help',
  },
  {
    id: 'ses',
    name: 'Amazon SES',
    host: '',
    hostPlaceholder: 'email-smtp.<region>.amazonaws.com',
    port: 587,
    security: 'starttls',
    helpKey: 'email-relay-preset-ses-help',
  },
  {
    id: 'mailgun',
    name: 'Mailgun',
    host: 'smtp.mailgun.org',
    port: 587,
    security: 'starttls',
    helpKey: 'email-relay-preset-mailgun-help',
  },
  {
    id: 'postmark',
    name: 'Postmark',
    host: 'smtp.postmarkapp.com',
    port: 587,
    security: 'starttls',
    helpKey: 'email-relay-preset-postmark-help',
  },
  { id: 'other', name: 'Other' },
];

/** The preset a saved host belongs to, so the right help shows on reload. */
export function presetForHost(host: string): SmtpPreset | undefined {
  const h = host.trim().toLowerCase();
  if (!h) return undefined;
  if (/^email-smtp\.[a-z0-9-]+\.amazonaws\.com$/.test(h)) return SMTP_PRESETS[2];
  return SMTP_PRESETS.find((p) => p.host && p.host === h);
}

/**
 * Mirror of the server's port/security check (`check_port_security`), so the
 * form reacts as the admin types. The server stays the authority at save.
 */
export function portSecurityCheck(
  port: number | null,
  security: SmtpSecurity,
): { level: 'ok' | 'warn' | 'error'; key?: string } {
  if (port === 465 && security !== 'tls') {
    return { level: 'error', key: 'email-relay-port-465-needs-tls' };
  }
  if (port === 587 && security === 'tls') {
    return { level: 'error', key: 'email-relay-port-587-needs-starttls' };
  }
  if (port === 25) return { level: 'warn', key: 'email-relay-port-25-warning' };
  if (security === 'plaintext') return { level: 'warn', key: 'email-relay-plaintext-warning' };
  return { level: 'ok' };
}

/** The usual port for a security setting. */
export function defaultPort(security: SmtpSecurity): number {
  return security === 'tls' ? 465 : 587;
}
