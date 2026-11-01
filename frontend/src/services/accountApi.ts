/**
 * A person's own account, as the settings that both apps share call it:
 * confirming it's them and their sessions. The agent app and the portal each
 * supply one (`useAccountApi`); the components never know which.
 *
 * The portal reaches the server's `/account` routes. The agent app keeps its
 * `/auth/*` routes for now: the native app bundles it and talks to servers
 * older than `/api/account`.
 */
import type { AxiosInstance } from 'axios';
import authService, {
  type RevokeOtherSessionsResult,
  type SessionInfo,
} from '@nosdesk/core/services/authService';
import passkeyService from '@/services/passkeyService';
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
};

/** The server's `/account` routes, through `client` (the portal's). */
export function createAccountApi(client: AxiosInstance): AccountApi {
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
  };
}
