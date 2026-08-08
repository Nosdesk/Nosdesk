import type { BrowserContext, Page } from '@playwright/test'

/** Demo account seeded by `make seed-demo`. A member, deliberately: it is the
 *  least-privileged account that can still reach the project views, so the
 *  specs exercise what an ordinary user sees. Agents are gated behind one-time
 *  MFA enrolment, which is not worth automating for layout assertions. */
export const DEMO_EMAIL = 'noah@demo.nosdesk.test'
export const DEMO_PASSWORD = 'Demo1234!'

/** Demo projects. `WITH_TICKETS` has tickets spread across several columns,
 *  which is what makes the landing and drag specs meaningful. */
export const PROJECT_WITH_TICKETS = 3
export const PROJECT_SPARSE = 2

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
 */
export async function gotoAndSettle(page: Page, path: string): Promise<void> {
  await page.goto(path, { waitUntil: 'domcontentloaded' })
  await page.waitForTimeout(4000)
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

/** The first card fully on screen, or null when none is — which is itself
 *  meaningful on a phone, where a board can open on empty columns.
 *
 *  Selected by `data-card-id`, a stable hook on the card root. An earlier
 *  version matched on element geometry and silently started finding nothing
 *  when the cards moved and the incidental wrapper sizes changed. */
export function visibleCard(page: Page): Promise<BoardCard | null> {
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
