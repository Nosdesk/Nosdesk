/**
 * Cache key for a single ticket's detail payload (Pinia Colada). Shared so every
 * consumer that reads or invalidates the ticket-detail entry uses the identical
 * key and they can never drift.
 *
 * (Formerly co-located with the /tickets/:id route Data Loader, which was removed
 * along with the experimental DataLoaderPlugin; the views are pool-native and the
 * two list loaders were redundant.)
 */
export function ticketDetailKey(
  id: number | string,
): readonly (string | number)[] {
  return ['tickets', 'detail', Number(id)] as const
}
