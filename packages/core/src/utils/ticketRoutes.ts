/**
 * A ticket's routes in the web apps, built in one place. People know a ticket
 * by its number: the agent app and the portal route `/tickets/<number>`. A
 * ticket known only by its id goes to `/tickets/id/<id>`, where the agent app
 * looks the number up and redirects. `frontend/scripts/check-ticket-routes.mjs`
 * refuses a ticket route built anywhere else, so an id never lands where a
 * number belongs. Pool-aware paths are in `ticketPaths.ts`.
 */

/** `/tickets/<number>`. */
export function ticketRoute(number: number): string {
  return `/tickets/${number}`
}

/** `/tickets/id/<id>`, for a ticket known only by its id (agent app). */
export function ticketRouteById(id: number): string {
  return `/tickets/id/${id}`
}
