// Where sign-in returns the requester to. A page that needs a session sends
// them to /login?redirect=<that page>; once signed in they land back there.

import type { LocationQuery, LocationQueryRaw } from 'vue-router'

/** Where sign-in lands when there is nowhere better to go. */
export const SIGNED_IN_HOME = '/tickets'

/**
 * The page to open after signing in. Only a path within the portal is
 * followed: one starting with a single `/`, no backslash or control
 * character, and not the sign-in page itself. Anything else lands on
 * {@link SIGNED_IN_HOME}.
 */
export function signInDestination(query: LocationQuery): string {
  const redirect = query.redirect
  if (typeof redirect !== 'string') return SIGNED_IN_HOME
  if (!redirect.startsWith('/') || redirect.startsWith('//')) return SIGNED_IN_HOME
  if (/[\\\u0000-\u001f\u007f]/.test(redirect)) return SIGNED_IN_HOME
  if (/^\/login(?:[/?#]|$)/i.test(redirect)) return SIGNED_IN_HOME
  return redirect
}

/** The sign-in page's query for a requester who was on `fullPath`. */
export function signInQuery(fullPath: string): LocationQueryRaw {
  return signInDestination({ redirect: fullPath }) === SIGNED_IN_HOME ? {} : { redirect: fullPath }
}
