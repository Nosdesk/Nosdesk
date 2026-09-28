// The Microsoft Teams personal tab (served at `/teams`, see backend
// `handlers::teams`): the portal inside Teams, Outlook and the Microsoft 365
// app.
//
// Teams signs the person in: nested app authentication gives us an access
// token for the workspace's own Entra app without a prompt, and the backend
// trades it for a short portal token held in memory. TeamsJS and MSAL are
// loaded only here, so the portal and the widget don't carry them.

import { ref } from 'vue'
import type { IPublicClientApplication } from '@azure/msal-browser'

export interface TeamsContext {
  /** The person's Entra tenant. */
  tenantId: string
  loginHint: string | null
  /** `default`, `dark` or `contrast`. */
  theme: string
  locale: string | null
  /** A deep link's target inside the tab (`ticket-12`, `approval-12`). */
  subPageId: string | null
}

/** Why the tab can't sign the person in on its own. */
export type TeamsProblem =
  /** Not running inside a Microsoft 365 host. */
  | 'not-in-teams'
  /** The workspace hasn't turned the tab on. */
  | 'off'
  /** This client can't do nested app authentication. */
  | 'unsupported'
  /** The person has to agree (consent) first: needs a click. */
  | 'consent'
  /** Signed in with Microsoft, but no account here (domain not listed). */
  | 'not-set-up'
  | 'failed'

/** The last sign-in's problem, for the status screen. */
export const teamsProblem = ref<TeamsProblem | null>(null)

type TeamsJs = typeof import('@microsoft/teams-js')

let teams: TeamsJs | null = null
let context: TeamsContext | null = null
let msal: IPublicClientApplication | null = null
let scope: string | null = null

/** Connect to the host. Null when not inside Teams (or it doesn't answer). */
export async function initTeams(): Promise<TeamsContext | null> {
  if (context) return context
  try {
    teams = await import('@microsoft/teams-js')
    await Promise.race([
      teams.app.initialize(),
      new Promise((_, reject) => setTimeout(() => reject(new Error('timeout')), 10_000)),
    ])
    const ctx = await teams.app.getContext()
    context = {
      tenantId: ctx.user?.tenant?.id ?? '',
      loginHint: ctx.user?.loginHint ?? null,
      theme: ctx.app.theme ?? 'default',
      locale: ctx.app.locale ?? null,
      subPageId: ctx.page.subPageId || null,
    }
    return context
  } catch {
    teams = null
    return null
  }
}

/** Tell Teams the tab has drawn (it shows its own loading indicator till then). */
export function notifyTeamsReady(): void {
  if (!teams) return
  teams.app.notifyAppLoaded()
  teams.app.notifySuccess()
}

/** Follow Teams' theme as the person changes it. */
export function onTeamsThemeChange(handler: (theme: string) => void): void {
  teams?.app.registerOnThemeChangeHandler(handler)
}

let bearer: string | null = null

/** The signed-in person's portal token, if any. */
export function teamsBearer(): string | null {
  return bearer
}

async function client(): Promise<IPublicClientApplication | 'off' | 'unsupported'> {
  if (msal) return msal
  if (!teams || !context) return 'unsupported'
  const info = await fetch('/api/portal/auth/teams')
    .then((r) => (r.ok ? r.json() : null))
    .catch(() => null)
  if (!info?.enabled) return 'off'
  if (!teams.nestedAppAuth.isNAAChannelRecommended()) return 'unsupported'
  const { createNestablePublicClientApplication } = await import('@azure/msal-browser')
  msal = await createNestablePublicClientApplication({
    auth: {
      clientId: info.client_id,
      // The workspace's app lives in the person's own tenant.
      authority: `https://login.microsoftonline.com/${context.tenantId}`,
    },
  })
  scope = info.scope
  return msal
}

async function exchange(token: string): Promise<TeamsProblem | null> {
  const res = await fetch('/api/portal/auth/teams/session', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ token }),
  }).catch(() => null)
  if (!res) return 'failed'
  if (!res.ok) {
    const body = await res.json().catch(() => null)
    return body?.code === 'teams_domain' ? 'not-set-up' : 'failed'
  }
  const data = (await res.json()) as { access_token?: string }
  bearer = data.access_token ?? null
  return bearer ? null : 'failed'
}

let signingIn: Promise<TeamsProblem | null> | null = null

/**
 * Sign in without a prompt. Null on success. `interactive` (from a click) may
 * show Microsoft's consent prompt; without it, a needed prompt comes back as
 * `consent` so the tab can offer a button.
 */
export function signInTeams(interactive = false): Promise<TeamsProblem | null> {
  signingIn ??= (async (): Promise<TeamsProblem | null> => {
    if (!context) return 'not-in-teams'
    const pca = await client()
    if (pca === 'off' || pca === 'unsupported') return pca
    const request = {
      scopes: [scope!],
      loginHint: context.loginHint ?? undefined,
    }
    try {
      const account =
        pca.getAccount({ tenantId: context.tenantId, loginHint: context.loginHint ?? undefined }) ??
        undefined
      const result = await pca.acquireTokenSilent({ ...request, account })
      return await exchange(result.accessToken)
    } catch (e) {
      const { InteractionRequiredAuthError } = await import('@azure/msal-browser')
      if (!(e instanceof InteractionRequiredAuthError)) return 'failed'
      if (!interactive) return 'consent'
      try {
        const result = await pca.acquireTokenPopup(request)
        return await exchange(result.accessToken)
      } catch {
        return 'consent'
      }
    }
  })()
    .then((problem) => {
      teamsProblem.value = problem
      return problem
    })
    .finally(() => {
      signingIn = null
    })
  return signingIn
}

/** Where a deep link points inside the portal. */
export function routeForSubPage(subPageId: string | null): string {
  const match = subPageId?.match(/^(ticket|approval)-(\d+)$/)
  if (!match) return '/tickets'
  return match[1] === 'ticket' ? `/tickets/${match[2]}` : `/approvals/${match[2]}`
}

/** The portal theme for a Teams theme. */
export function portalTheme(teamsTheme: string): string {
  if (teamsTheme === 'dark') return 'dark'
  if (teamsTheme === 'contrast') return 'pure-black'
  return 'light'
}
