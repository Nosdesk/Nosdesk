/**
 * "Confirm it's you" before a high-risk action. The server marks the caller's
 * own session as confirmed for a few minutes; while that holds, the action
 * endpoints accept an empty credential. See `useStepUp` for the client flow.
 */
import apiClient from '@nosdesk/core/apiClient';

export type StepUpMethod = 'password' | 'passkey' | 'provider' | 'code';

export interface StepUpProvider {
  provider_type: 'oidc' | 'microsoft';
  name: string;
}

export interface StepUpStatus {
  /** RFC 3339 UTC; null when this session has not confirmed recently. */
  confirmed_until: string | null;
  window_minutes: number;
  /** Best first; may be empty. */
  methods: StepUpMethod[];
  provider: StepUpProvider | null;
  email: string | null;
}

export interface StepUpConfirmed {
  confirmed_until: string;
}

export type StepUpCredential = { password: string } | { mfa_code: string };

/** The popup's outcome, as `/auth/confirmed?result=` carries it. */
export type StepUpPopupResult =
  | 'ok'
  | 'cancelled'
  | 'wrong_account'
  | 'not_fresh'
  | 'signed_out'
  | 'failed';

export const STEP_UP_POPUP_RESULTS: readonly StepUpPopupResult[] = [
  'ok',
  'cancelled',
  'wrong_account',
  'not_fresh',
  'signed_out',
  'failed',
];

/** Where the popup starts. A page load, not an API call. */
export const STEP_UP_PROVIDER_URL = '/api/auth/step-up/provider';
/** The popup's window name, so a second click reuses the same window. */
export const STEP_UP_POPUP_NAME = 'nosdesk-step-up';
export const STEP_UP_POPUP_FEATURES = 'popup,width=520,height=720';
/** The channel `/auth/confirmed` reports the popup's outcome on. */
export const STEP_UP_CHANNEL = 'nosdesk-step-up';

export interface StepUpChannelMessage {
  type: 'step-up';
  result: StepUpPopupResult;
}

export const stepUpService = {
  async status(): Promise<StepUpStatus> {
    const { data } = await apiClient.get<StepUpStatus>('/auth/step-up');
    return data;
  },

  async confirm(credential: StepUpCredential): Promise<StepUpConfirmed> {
    const { data } = await apiClient.post<StepUpConfirmed>('/auth/step-up', credential);
    return data;
  },
};

export default stepUpService;
