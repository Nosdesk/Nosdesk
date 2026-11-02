import apiClient from '../apiClient';

/** Where this workspace's public portal lives and what it offers. */
export interface WorkspacePortal {
  /** The portal's origin, or null when unknown (use the page's own origin). */
  portal_url: string | null;
  request_form_enabled: boolean;
  public_docs_enabled: boolean;
  help_page_enabled: boolean;
}

export async function getWorkspacePortal(): Promise<WorkspacePortal> {
  const { data } = await apiClient.get<WorkspacePortal>('/workspace/portal');
  return data;
}

/**
 * A one-time address that opens the portal signed in as the caller (the
 * native app's hand-off to the system browser, self-hosted). Single use and
 * short-lived: ask for it right before opening it.
 */
export async function requestPortalHandoff(next?: string): Promise<string> {
  const { data } = await apiClient.post<{ url: string }>('/me/portal-handoff', next ? { next } : {});
  return data.url;
}
