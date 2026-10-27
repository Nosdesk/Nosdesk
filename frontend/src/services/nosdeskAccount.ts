/**
 * Hand-offs from a Cloud workspace to Nosdesk itself: the account dashboard's
 * billing page and the support address. Self-hosted instances have neither;
 * callers gate on `isHostedDeploymentRef`.
 */
import { getControlPlaneUrl } from '@nosdesk/core/services/instanceConfig';

/** Where Cloud customers write for account, plan and billing questions. */
export const NOSDESK_SUPPORT_EMAIL = 'support@nosdesk.com';

/** The billing page on the control-plane dashboard. It acts on the signed-in
 *  person's current account, so there is nothing per workspace to add. */
export const CONTROL_PLANE_BILLING_PATH = '/account/billing';

/** The control-plane billing page, or `''` when the control-plane URL is
 *  unknown (self-hosted, or the config has not resolved). */
export function controlPlaneBillingUrl(): string {
  const cp = getControlPlaneUrl().replace(/\/$/, '');
  return cp ? `${cp}${CONTROL_PLANE_BILLING_PATH}` : '';
}

/** Open the billing page in the user's real browser (a new tab on web, the
 *  system browser in the native shell). No-op when the URL is unknown. */
export async function openControlPlaneBilling(): Promise<void> {
  const url = controlPlaneBillingUrl();
  if (!url) return;
  const { openExternalUrl } = await import('@/platform');
  await openExternalUrl(url);
}
