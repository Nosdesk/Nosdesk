/**
 * The landing views' fall-through. My Open and My day show All Active
 * in their place when nothing open is assigned to you but the workspace
 * has open work, so an agent never lands on a blank screen. Tickets you
 * closed don't count as work, so a long list of them can't hide the
 * open queue.
 */
import type { CardData } from './types'
import { MY_DAY_VIEW, MY_OPEN_VIEW } from './builtinViews'

/** The views that fall through to All Active. */
export const FALL_THROUGH_VIEW_IDS: ReadonlySet<string> = new Set([MY_OPEN_VIEW.id, MY_DAY_VIEW.id])

/** Whether `viewId` shows All Active instead. The predicates are My
 * Open's and All Active's, built by the caller so it can reuse them. */
export function fallsThroughToAllActive(
  viewId: string,
  cards: readonly CardData[],
  myOpen: (card: CardData) => boolean,
  allActive: (card: CardData) => boolean,
): boolean {
  if (!FALL_THROUGH_VIEW_IDS.has(viewId)) return false
  // .some() short-circuits on the first match instead of
  // materialising a full filtered array just to read its length.
  if (cards.some(myOpen)) return false
  return cards.some(allActive)
}
