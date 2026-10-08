/**
 * What the People list shows, by who is looking. Staff work from the full
 * roster: addresses, roles, load and join dates. Anyone else gets only other
 * people's names and avatars from the server, so the list shows just that,
 * with no columns, filters or groupings that would read as empty.
 */

export type PeopleColumn =
  | 'user'
  | 'email'
  | 'role'
  | 'open_ticket_count'
  | 'device_count'
  | 'created_at'

export type PeopleGroupAxis = 'role' | 'status' | 'joined'

export type PeopleFacet = 'name' | 'role'

/** The columns, in order. Requesters (end-users) have no role, tickets or
 *  assets, so their roster is name, address and join date. */
export function peopleColumns(isStaff: boolean, isRequesters: boolean): PeopleColumn[] {
  if (!isStaff) return ['user']
  if (isRequesters) return ['user', 'email', 'created_at']
  return ['user', 'email', 'role', 'open_ticket_count', 'device_count', 'created_at']
}

export function peopleGroupAxes(isStaff: boolean): PeopleGroupAxis[] {
  return isStaff ? ['role', 'status', 'joined'] : ['status']
}

export function peopleFacets(isStaff: boolean): PeopleFacet[] {
  return isStaff ? ['name', 'role'] : ['name']
}
