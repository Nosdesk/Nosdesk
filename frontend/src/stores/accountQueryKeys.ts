/**
 * Query keys whose data belongs to the signed-in account rather than to a
 * workspace. A workspace switch keeps these cached (and refetches them in the
 * background) while it drops every workspace-scoped query; sign-out drops them
 * too. Each key here also carries the user's uuid, so accounts never share an
 * entry.
 *
 * A leaf module, so `workspaceReset` can import it statically without pulling
 * in the stores that own the queries.
 */

/** `GET /api/me/workspaces`: the workspaces the account belongs to. */
export const MY_WORKSPACES_KEY = ['my-workspaces'] as const;

export const ACCOUNT_SCOPED_QUERY_KEYS: ReadonlyArray<readonly string[]> = [MY_WORKSPACES_KEY];
