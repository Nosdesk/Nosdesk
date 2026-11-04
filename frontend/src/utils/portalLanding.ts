/**
 * A sign-in at the app that the server answered by signing the person in to
 * the portal instead (a requester on self-hosted): go there. Returns whether
 * it did.
 *
 * The web app gets the portal's cookies and a same-origin path to follow; only
 * such a path is followed. The native app gets no session, only a one-time
 * address (`portal_handoff`) that signs the system browser in to the portal,
 * opened there at once, and the portal's plain address (`portal_url`) for the
 * requester screen to open again later.
 */
export function followPortalLanding(data: unknown): boolean {
  const body = (data ?? {}) as { portal?: unknown; portal_handoff?: unknown; portal_url?: unknown }
  if (isWebAddress(body.portal_handoff)) {
    const portal = isWebAddress(body.portal_url) ? body.portal_url : undefined
    void import('@/platform').then(({ openExternalUrl }) => openExternalUrl(body.portal_handoff as string))
    void import('@/router').then(({ default: router }) =>
      router.replace({ name: 'requester-portal', query: portal ? { portal } : {} }),
    )
    return true
  }
  const to = body.portal
  if (typeof to !== 'string' || !to.startsWith('/') || to.startsWith('//') || to.startsWith('/\\')) {
    return false
  }
  window.location.assign(to)
  return true
}

/** An absolute http(s) address. */
export function isWebAddress(value: unknown): value is string {
  if (typeof value !== 'string') return false
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:'
  } catch {
    return false
  }
}
