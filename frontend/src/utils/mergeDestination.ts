/** What the merge destination rule needs from a ticket. */
export interface MergeCandidate {
  id: number
  /** ISO timestamp; when absent the lowest id is the oldest (ids are
   *  monotonic). */
  created?: string
}

/**
 * The ticket a merge goes into by default: `preferredId` when it is one of
 * `tickets` (the row right-clicked, or the ticket last ticked on), else the
 * oldest. Selection order isn't used: select-all and shift-ranges add in list
 * order, so it would follow the sort.
 */
export function mergeDestination<T extends MergeCandidate>(
  tickets: readonly T[],
  preferredId?: number | null,
): T | null {
  const preferred = preferredId == null ? undefined : tickets.find((t) => t.id === preferredId)
  if (preferred) return preferred
  if (tickets.length === 0) return null
  return [...tickets].sort((a, b) => {
    if (a.created && b.created) return a.created.localeCompare(b.created)
    return a.id - b.id
  })[0]
}
