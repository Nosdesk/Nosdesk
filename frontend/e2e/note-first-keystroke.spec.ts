import { test, expect, type APIRequestContext, type Page } from '@playwright/test'
import { AUTH_STATE } from './helpers'

/**
 * Typing into a note just opened keeps what it already says.
 *
 * A note's content reaches its editor after the editor mounts: from IndexedDB
 * on a warm open, or from the server in a fresh browser. It used to arrive as
 * the selection: the whole note sat selected, and a keystroke that landed
 * before a click's caret took effect, or after focusing the editor from the
 * keyboard, replaced all of it for every viewer.
 */
test.skip(({ hasTouch }) => hasTouch, 'pointer and keyboard input')

const NOTE = 'Printer on level 3 jams on duplex jobs'
/** What each step types. Each is taken out again, latest first, to check the
 *  note around them is intact wherever the caret put them. */
const TYPED = [' (clicked cold)', ' (clicked warm)', ' (keyboard)']

/** The ticket's note editor (not the reply box, which is also ProseMirror). */
const noteEditor = (page: Page) => page.locator('.ProseMirror:not(.simple-editor-content)').first()

/** The note's text, without other people's cursor labels and without what
 *  the test typed. */
const untyped = (page: Page) =>
  noteEditor(page).evaluate((el, typed) => {
    const copy = el.cloneNode(true) as HTMLElement
    copy.querySelectorAll('.ProseMirror-yjs-cursor').forEach((c) => c.remove())
    let text = copy.textContent ?? ''
    for (const t of [...typed].reverse()) text = text.replace(t, '')
    return text
  }, TYPED)

async function csrfHeaders(page: Page): Promise<Record<string, string>> {
  const csrf = (await page.context().cookies()).find((c) => c.name === 'csrf_token')?.value ?? ''
  return { 'X-CSRF-Token': csrf }
}

async function createTicket(request: APIRequestContext, headers: Record<string, string>) {
  const states = await (await request.get('/api/workflow-states')).json()
  const state = (states.states ?? states).find((s: { is_default: boolean }) => s.is_default)
  const created = await request.post('/api/tickets', {
    headers,
    data: { title: 'E2E note first keystroke', priority: 'medium', workflow_state_id: state.id },
  })
  expect(created.ok()).toBe(true)
  return (await created.json()) as { id: number; number?: number }
}

test('typing into a freshly opened note keeps its text', async ({ page, browser }) => {
  const headers = await csrfHeaders(page)
  const ticket = await createTicket(page.request, headers)
  const path = `/tickets/${ticket.number ?? ticket.id}`
  const fresh = await browser.newContext({ storageState: AUTH_STATE })
  const check = await browser.newContext({ storageState: AUTH_STATE })
  try {
    // Write the note in one browser.
    await page.goto(path, { waitUntil: 'domcontentloaded' })
    await noteEditor(page).click()
    await expect(noteEditor(page)).toBeFocused()
    await page.keyboard.type(NOTE)
    await expect(noteEditor(page)).toContainText(NOTE)

    // A fresh browser: the content arrives from the server. Click and type
    // at once.
    const other = await fresh.newPage()
    await other.goto(path, { waitUntil: 'domcontentloaded' })
    await expect(noteEditor(other)).toContainText(NOTE, { timeout: 20_000 })
    await noteEditor(other).click()
    await other.keyboard.type(TYPED[0])
    await expect(noteEditor(other)).toContainText(TYPED[0])
    await expect.poll(() => untyped(other)).toContain(NOTE)

    // Reopened in the same browser: the content now comes from its cache.
    await other.reload({ waitUntil: 'domcontentloaded' })
    await expect(noteEditor(other)).toContainText(TYPED[0], { timeout: 20_000 })
    await noteEditor(other).click()
    await other.keyboard.type(TYPED[1])
    await expect(noteEditor(other)).toContainText(TYPED[1])
    await expect.poll(() => untyped(other)).toContain(NOTE)

    // Reopened again and reached from the keyboard, with no click at all.
    await other.reload({ waitUntil: 'domcontentloaded' })
    await expect(noteEditor(other)).toContainText(TYPED[1], { timeout: 20_000 })
    await noteEditor(other).focus()
    await other.keyboard.type(TYPED[2])
    await expect(noteEditor(other)).toContainText(TYPED[2])
    await expect.poll(() => untyped(other)).toContain(NOTE)

    // The first browser saw it all, live.
    for (const typed of TYPED) await expect(noteEditor(page)).toContainText(typed)
    await expect.poll(() => untyped(page)).toContain(NOTE)

    // And it was saved: a third browser, with nothing cached, loads the same.
    const saved = await check.newPage()
    await saved.goto(path, { waitUntil: 'domcontentloaded' })
    for (const typed of TYPED) {
      await expect(noteEditor(saved)).toContainText(typed, { timeout: 20_000 })
    }
    await expect.poll(() => untyped(saved)).toContain(NOTE)
  } finally {
    await fresh.close()
    await check.close()
    await page.request.delete(`/api/tickets/${ticket.id}`, { headers })
  }
})
