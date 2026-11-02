/**
 * A sign-in at the app that the server answered by signing the person in to
 * the portal instead (a requester on self-hosted): go there. Returns whether
 * it did. Only a same-origin path is followed.
 */
export function followPortalLanding(data: unknown): boolean {
  const to = (data as { portal?: unknown } | null | undefined)?.portal;
  if (typeof to !== 'string' || !to.startsWith('/') || to.startsWith('//') || to.startsWith('/\\')) {
    return false;
  }
  window.location.assign(to);
  return true;
}
