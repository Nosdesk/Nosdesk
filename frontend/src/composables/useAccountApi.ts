import { inject, type InjectionKey } from 'vue';
import { agentAccountApi, type AccountApi } from '@/services/accountApi';

/** Which app's account routes the shared settings call. The portal provides
 *  its own at startup; without one, it is the agent app's. */
export const ACCOUNT_API: InjectionKey<AccountApi> = Symbol('account-api');

export function useAccountApi(): AccountApi {
  return inject(ACCOUNT_API, agentAccountApi);
}
