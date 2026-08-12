import type { BrowserContext, Page } from '@playwright/test'

/** Demo account seeded by `make seed-demo`. A member, deliberately: it is the
 *  least-privileged account that can still reach the project views, so the
 *  specs exercise what an ordinary user sees. Agents are gated behind one-time
 *  MFA enrolment, which is not worth automating for layout assertions. */
export const DEMO_EMAIL = 'noah@demo.nosdesk.test'
export const DEMO_PASSWORD = 'Demo1234!'

/**
 * Demo projects, split by SURFACE, because that is what determines whether one
 * spec's writes can hurt another's assertions.
 *
 * Kanban drags move a card between columns. That is harmless to board
 * assertions — a card in any column is still on the board — but it is fatal to
 * timeline assertions, because cards drift monotonically toward `done` and
 * terminal work is filtered out of the timeline. Repeated runs emptied the
 * timeline of whichever project the drag spec shared with it.
 *
 * So the boards and the timeline get different projects. Restoring the card
 * through a second drag was tried and abandoned: the board auto-pans mid-drag,
 * so any drop coordinate computed beforehand is stale, and the restore missed
 * by a column. Housekeeping should not be a second flaky gesture.
 *
 * The shapes these names promise are asserted once, up front, by
 * `seed-contract.spec.ts` — so a seed change fails there with a plain message
 * instead of surfacing as a timeout inside an unrelated spec.
 *
 * Shapes are stated as what `DEMO_EMAIL` can SEE, not what the database holds.
 * That account is a member, so it sees only its own tickets: project 2 holds 16
 * but shows this user 3, and only project 3's planned work belongs to them.
 * Reading the raw table instead is what made a first pass at this hand the
 * mutable project to one whose timeline is empty for the test user.
 */
/** Board specs, including the drags. They rearrange its columns freely; no
 *  assertion here depends on which column a card is in. */
export const PROJECT_BOARD = 2
/** Timeline specs. The only project whose planned blocks are visible to this
 *  user, so nothing may push its tickets into a terminal state. The reschedule
 *  spec restores the dates it moves. */
export const PROJECT_TIMELINE = 3
/** No scheduled work, so the timeline's empty state is reachable. */
export const PROJECT_SPARSE = 1

export async function login(page: Page): Promise<void> {
  await page.goto('/login', { waitUntil: 'domcontentloaded' })
  await page.waitForSelector('input[type="email"]')
  await page.fill('input[type="email"]', DEMO_EMAIL)
  await page.fill('input[type="password"]', DEMO_PASSWORD)
  await page.click('button[type="submit"]')
  // Not `networkidle`: the app holds an SSE connection open, so the network
  // never goes idle and waiting for it always times out.
  await page.waitForURL((u) => !u.pathname.includes('/login'))
}

/**
 * Go to a route and wait for the sync pool to have filled in.
 *
 * The pool loads AFTER mount, so asserting immediately reads an empty board.
 * This is not cosmetic: the kanban landing scroll originally hooked `onMounted`
 * and silently did nothing for exactly this reason.
 *
 * Pass `waitFor` whenever the assertion needs data on screen. The bare form is
 * a fixed settle, which is a duration standing in for a condition: it is enough
 * on a laptop and not on a loaded CI runner, where the landing-scroll test
 * measured an empty board and failed both attempts while passing locally.
 * Layout assertions that hold on an empty view can keep using the bare form.
 */
export async function gotoAndSettle(page: Page, path: string, waitFor?: string): Promise<void> {
  await page.goto(path, { waitUntil: 'domcontentloaded' })
  if (waitFor === undefined) {
    await page.waitForTimeout(4000)
    return
  }
  await page.waitForSelector(waitFor, { timeout: 30_000 })
  // The landing scroll runs on the tick after the cards arrive, so the element
  // existing is not yet the board having settled where it opens.
  await page.waitForTimeout(750)
}

/** Real touch events through CDP. Playwright's `touchscreen` only taps, and a
 *  synthesised pointer event would bypass the browser's gesture arbitration —
 *  which is the very thing these specs are testing. */
export class Touch {
  private constructor(private readonly cdp: Awaited<ReturnType<BrowserContext['newCDPSession']>>) {}

  static async create(context: BrowserContext, page: Page): Promise<Touch> {
    return new Touch(await context.newCDPSession(page))
  }

  start(x: number, y: number): Promise<unknown> {
    return this.cdp.send('Input.dispatchTouchEvent', {
      type: 'touchStart',
      touchPoints: [{ x, y, id: 1 }],
    })
  }

  move(x: number, y: number): Promise<unknown> {
    return this.cdp.send('Input.dispatchTouchEvent', {
      type: 'touchMove',
      touchPoints: [{ x, y, id: 1 }],
    })
  }

  end(): Promise<unknown> {
    return this.cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] })
  }

  /** Drag in steps, so the browser sees a gesture rather than a teleport. */
  async drag(fromX: number, y: number, dx: number, page: Page, steps = 10): Promise<void> {
    for (let i = 1; i <= steps; i++) {
      await this.move(fromX + (dx * i) / steps, y)
      await page.waitForTimeout(16)
    }
  }
}

/** The kanban's horizontal scroll container. */
export const BOARD = '.kanban-board'

/** Scroll offset of the board, for asserting pan-versus-drag. */
export function boardScrollLeft(page: Page): Promise<number> {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel)
    return el ? Math.round(el.scrollLeft) : -1
  }, BOARD)
}

/**
 * Which way the board can still scroll, as a drag delta.
 *
 * The board lands on the first populated column, which is often the far right,
 * so a hardcoded leftward swipe can start pinned at maximum scroll and appear
 * not to pan at all. Negative drags the content left (scrollLeft grows).
 */
export function panDirection(page: Page): Promise<number> {
  return page.evaluate((sel) => {
    const el = document.querySelector(sel)
    if (!el) return -150
    const max = el.scrollWidth - el.clientWidth
    return el.scrollLeft >= max - 1 ? 150 : -150
  }, BOARD)
}

export interface BoardCard {
  /** Ticket id, so a move can be asserted about THIS card. Comparing "the
   *  first visible card" before and after silently compares two different
   *  cards once a drag reorders the board. */
  id: string
  x: number
  y: number
}

/** A planned block in the mobile vertical timeline. The E2E drag test uses a
 * ticket with an authored start so its forward-and-back gesture restores the
 * exact original dates, rather than promoting an inferred start as a side
 * effect of the coverage itself. */
export interface TimelineCard {
  id: string
  x: number
  y: number
}

export async function visibleTimelineCard(page: Page): Promise<TimelineCard | null> {
  const card = page.locator('[data-timeline-card-id][data-timeline-has-start-date="true"]').first()
  if ((await card.count()) === 0) return null
  await card.scrollIntoViewIfNeeded()
  const box = await card.boundingBox()
  if (!box) return null
  return {
    id: await card.getAttribute('data-timeline-card-id') ?? '',
    x: Math.round(box.x + box.width / 2),
    y: Math.round(box.y + box.height / 2),
  }
}

/**
 * A card to drive a gesture against, scrolling it into view if needed.
 *
 * Scrolling matters because the specs run against shared demo data that drifts:
 * a card can end up in a column that is off-screen, and a spec asserting drag
 * BEHAVIOUR should not fail because of where a previous run left the data.
 * Whether a card is on screen *without* scrolling is a separate question, and
 * `project-views-mobile.spec.ts` asserts that one directly.
 */
export async function visibleCard(page: Page): Promise<BoardCard | null> {
  const found = await firstOnScreenCard(page)
  if (found) return found
  // Bring the first column that has a card into view, then look again.
  await page.evaluate((sel) => {
    const board = document.querySelector(sel)
    const card = board?.querySelector('[data-card-id]')
    if (board && card) {
      const r = card.getBoundingClientRect()
      board.scrollLeft += r.left - board.getBoundingClientRect().left
    }
  }, BOARD)
  await page.waitForTimeout(600)
  return firstOnScreenCard(page)
}

/** Selected by `data-card-id`, a stable hook on the card root. An earlier
 *  version matched on element geometry and silently started finding nothing
 *  when the cards moved and the incidental wrapper sizes changed. */
function firstOnScreenCard(page: Page): Promise<BoardCard | null> {
  return page.evaluate((sel) => {
    const board = document.querySelector(sel)
    if (!board) return null
    const card = [...board.querySelectorAll('[data-card-id]')].find((el) => {
      const r = el.getBoundingClientRect()
      return (
        r.width > 0 && r.height > 0 &&
        r.left >= 0 && r.right <= window.innerWidth &&
        r.top > 0 && r.bottom < window.innerHeight
      )
    })
    if (!card) return null
    const r = card.getBoundingClientRect()
    return {
      id: card.getAttribute('data-card-id') ?? '',
      x: Math.round(r.left + r.width / 2),
      y: Math.round(r.top + r.height / 2),
    }
  }, BOARD)
}

/**
 * Which column a specific ticket is in, by id.
 *
 * Resolved from the COLUMN down rather than from the card up: walking up from a
 * card and taking the first ancestor in a plausible width range picked up the
 * wrong heading and reported a column the card was never in.
 */
/**
 * Locate one specific card by id, scrolling it into view first.
 *
 * `visibleCard` finds *a* card; this finds *the* card, which is what a restore
 * step needs — after a move it sits in a different column, often off-screen.
 */
export async function cardById(page: Page, id: string): Promise<BoardCard | null> {
  const el = page.locator(`[data-card-id="${id}"]`).first()
  if ((await el.count()) === 0) return null
  await el.scrollIntoViewIfNeeded()
  const box = await el.boundingBox()
  if (!box) return null
  return { id, x: Math.round(box.x + box.width / 2), y: Math.round(box.y + box.height / 2) }
}

export function columnOfCard(page: Page, id: string): Promise<string | null> {
  return page.evaluate(
    ({ sel, id }) => {
      const board = document.querySelector(sel)
      if (!board) return null
      const inner = board.firstElementChild
      if (!inner) return null
      for (const column of [...inner.children]) {
        const heading = column.querySelector('h2, h3')?.textContent?.trim()
        if (!heading) continue
        if (column.querySelector(`[data-card-id="${id}"]`)) return heading
      }
      return null
    },
    { sel: BOARD, id },
  )
}

/**
 * Which way to drag a card so it lands on a column that exists: +1 right, -1
 * left. A card already in the last column has nowhere to go rightwards.
 *
 * Without this the drag specs pile every card into the rightmost column over
 * repeated runs against the same demo data, and then fail because the move they
 * ask for is impossible. Choosing the direction also keeps the data balanced:
 * cards oscillate instead of drifting to an edge.
 */
/**
 * Viewport x of a column's centre, by its heading, scrolling it into view.
 *
 * A restore step cannot reverse the forward gesture by the same pixel delta:
 * columns are ~74% of a phone viewport, so a fixed 200px nudge that was enough
 * to leave a column is not necessarily enough to re-enter the original one, and
 * the card lands one short. Targeting the column itself is width-independent.
 */
export async function columnCenterX(page: Page, heading: string): Promise<number | null> {
  return page.evaluate(
    ({ sel, heading }) => {
      const inner = document.querySelector(sel)?.firstElementChild
      if (!inner) return null
      for (const column of [...inner.children]) {
        const h = column.querySelector('h2, h3')?.textContent?.trim()
        if (h !== heading) continue
        const board = document.querySelector(sel) as HTMLElement
        const r = column.getBoundingClientRect()
        const br = board.getBoundingClientRect()
        // Bring it fully on screen so the drop coordinate is reachable.
        if (r.left < br.left || r.right > br.right) {
          board.scrollLeft += r.left - br.left - 8
        }
        const after = column.getBoundingClientRect()
        return Math.round(after.left + after.width / 2)
      }
      return null
    },
    { sel: BOARD, heading },
  )
}

export function dragDirection(page: Page, id: string): Promise<number> {
  return page.evaluate(
    ({ sel, id }) => {
      const inner = document.querySelector(sel)?.firstElementChild
      if (!inner) return 1
      const columns = [...inner.children]
      const index = columns.findIndex((c) => c.querySelector(`[data-card-id="${id}"]`))
      return index >= 0 && index === columns.length - 1 ? -1 : 1
    },
    { sel: BOARD, id },
  )
}
