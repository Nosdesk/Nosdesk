/**
 * A person's own account, as the settings that both apps share call it:
 * confirming it's them, their sessions, password, authenticator and
 * passkeys. The agent app and the portal each supply one (`useAccountApi`);
 * the components never know which.
 *
 * The portal reaches the server's `/account` routes. The agent app keeps its
 * `/auth/*` routes for now: the native app bundles it and talks to servers
 * older than `/api/account`. Once it moves, `agentAccountApi` becomes
 * `createAccountApi(apiClient)` and the old service calls go.
 */
import type { AxiosInstance } from 'axios';
import authService, {
  type MFAEnableRequest,
  type MFASetupData,
  type MFAStatusResponse,
  type MFAVerifyRequest,
  type RevokeOtherSessionsResult,
  type SessionInfo,
} from '@nosdesk/core/services/authService';
import passkeyService, {
  type PasskeyInfo,
  type PasskeyRegistrationResult,
} from '@/services/passkeyService';
import stepUpService, {
  type StepUpConfirmed,
  type StepUpCredential,
  type StepUpStatus,
} from '@/services/stepUpService';

export interface AccountApi {
  stepUp: {
    status(): Promise<StepUpStatus>;
    confirm(credential: StepUpCredential): Promise<StepUpConfirmed>;
    /** Runs the browser's passkey prompt, then confirms with its answer. */
    confirmWithPasskey(): Promise<StepUpConfirmed>;
  };
  sessions: {
    list(): Promise<SessionInfo[]>;
    revoke(sessionId: string): Promise<void>;
    /** Needs a confirmed session (or a credential with the request). */
    revokeOthers(credential?: StepUpCredential): Promise<RevokeOtherSessionsResult>;
  };
  password: {
    change(currentPassword: string, newPassword: string): Promise<void>;
  };
  mfa: {
    status(): Promise<MFAStatusResponse>;
    setup(): Promise<MFASetupData>;
    verify(request: MFAVerifyRequest): Promise<{ valid: boolean }>;
    enable(request: MFAEnableRequest): Promise<{ success: boolean; backup_codes: string[] }>;
    disable(password: string): Promise<{ success: boolean }>;
    regenerateBackupCodes(password: string): Promise<{ backup_codes: string[] }>;
  };
  passkeys: {
    list(): Promise<PasskeyInfo[]>;
    /** Runs the browser's passkey prompt, then saves the new passkey. */
    register(name?: string): Promise<PasskeyRegistrationResult>;
    rename(credentialId: string, name: string): Promise<boolean>;
    remove(credentialId: string, password: string): Promise<boolean>;
  };
}

/** The agent app's, over its existing services. */
export const agentAccountApi: AccountApi = {
  stepUp: {
    status: () => stepUpService.status(),
    confirm: (credential) => stepUpService.confirm(credential),
    confirmWithPasskey: () => passkeyService.confirmStepUp(),
  },
  sessions: {
    list: () => authService.getSessions(),
    revoke: (sessionId) => authService.revokeSession(sessionId),
    revokeOthers: (...credential) => authService.revokeAllOtherSessions(...credential),
  },
  password: {
    change: (currentPassword, newPassword) => authService.changePassword(currentPassword, newPassword),
  },
  mfa: {
    status: () => authService.getMFAStatus(),
    setup: () => authService.setupMFA(),
    verify: (request) => authService.verifyMFA(request),
    enable: (request) => authService.enableMFA(request),
    disable: (password) => authService.disableMFA(password),
    regenerateBackupCodes: (password) => authService.regenerateBackupCodes(password),
  },
  passkeys: {
    list: () => passkeyService.listPasskeys(),
    register: (name) => passkeyService.registerPasskey(name),
    rename: (credentialId, name) => passkeyService.renamePasskey(credentialId, name),
    remove: (credentialId, password) => passkeyService.deletePasskey(credentialId, password),
  },
};

/** The server's `/account` routes, through `client` (the portal's). */
export function createAccountApi(client: AxiosInstance): AccountApi {
  const passkeys = { client, base: '/account/passkeys' };
  return {
    stepUp: {
      async status() {
        return (await client.get<StepUpStatus>('/account/step-up')).data;
      },
      async confirm(credential) {
        return (await client.post<StepUpConfirmed>('/account/step-up', credential)).data;
      },
      confirmWithPasskey: () => passkeyService.confirmStepUp(client, '/account/step-up'),
    },
    sessions: {
      async list() {
        return (await client.get<{ sessions: SessionInfo[] }>('/account/sessions')).data.sessions;
      },
      async revoke(sessionId) {
        await client.delete(`/account/sessions/${encodeURIComponent(sessionId)}`);
      },
      async revokeOthers(credential) {
        const { data } = await client.delete<RevokeOtherSessionsResult>('/account/sessions/others', {
          data: credential ?? {},
        });
        return data ?? {};
      },
    },
    password: {
      async change(currentPassword, newPassword) {
        await client.post('/account/password', {
          current_password: currentPassword,
          new_password: newPassword,
        });
      },
    },
    mfa: {
      async status() {
        return (await client.get<MFAStatusResponse>('/account/mfa/status')).data;
      },
      async setup() {
        return (await client.post<MFASetupData>('/account/mfa/setup')).data;
      },
      async verify(request) {
        const { data } = await client.post('/account/mfa/verify-setup', request);
        return { valid: data.success === true };
      },
      async enable(request) {
        const { data } = await client.post('/account/mfa/enable', request);
        return { success: data.status === 'success', backup_codes: data.backup_codes || [] };
      },
      async disable(password) {
        const { data } = await client.post('/account/mfa/disable', { password });
        return { success: data.status === 'success' };
      },
      async regenerateBackupCodes(password) {
        const { data } = await client.post<{ backup_codes: string[] }>(
          '/account/mfa/regenerate-backup-codes',
          { password },
        );
        return data;
      },
    },
    passkeys: {
      list: () => passkeyService.listPasskeys(passkeys),
      register: (name) => passkeyService.registerPasskey(name, passkeys),
      rename: (credentialId, name) => passkeyService.renamePasskey(credentialId, name, passkeys),
      remove: (credentialId, password) => passkeyService.deletePasskey(credentialId, password, passkeys),
    },
  };
}
