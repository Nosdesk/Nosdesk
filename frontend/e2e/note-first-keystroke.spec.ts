import { test, expect, type Page } from '@playwright/test'
import { AUTH_STATE } from './helpers'

/**
 * Typing into a note just opened keeps what it already says.
 *
 * A note's content reaches its editor after the editor mounts (from IndexedDB
 * or the server). It used to arrive as the selection: the whole note sat
 * selected, and a keystroke that landed before the click's caret took effect
 * replaced all of it. Clicking and typing at once, as people and automation
 * both do, wiped the note for every viewer.
 *
 * A fresh browser context has an empty IndexedDB, so the content arrives over
 * the WebSocket, which is the path a sign-out (or a new device) takes.
 */
test.skip(({ hasTouch }) => hasTouch, 'pointer and keyboard input')

const NOTE = 'Printer on level 3 jams on duplex jobs'
const TYPED = ' (reported twice)'

/** The ticket's note editor (not the reply box, which is also ProseMirror). */
const noteEditor = (page: Page) => page.locator('.ProseMirror:not(.simple-editor-content)').first()

/** The note's text, without other people's cursor labels and with the typed
 *  addition taken out. */
const withoutTyped = (page: Page) =>
  noteEditor(page).evaluate((el, typed) => {
    const copy = el.cloneNode(true) as HTMLElement
    copy.querySelectorAll('.ProseMirror-yjs-cursor').forEach((c) => c.remove())
    return (copy.textContent ?? '').replace(typed, '')
  }, TYPED)

test('clicking into a freshly opened note and typing at once keeps its text', async ({ page, browser }) => {
  // A ticket of our own, so the note's content is known.
  const csrf = (await page.context().cookies()).find((c) => c.name === 'csrf_token')?.value ?? ''
  const headers = { 'X-CSRF-Token': csrf }
  const states = await (await page.request.get('/api/workflow-states')).json()
  const state = (states.states ?? states).find((s: { is_default: boolean }) => s.is_default)
  const created = await page.request.post('/api/tickets', {
    headers,
    data: { title: 'E2E note first keystroke', priority: 'medium', workflow_state_id: state.id },
  })
  expect(created.ok()).toBe(true)
  const ticket = await created.json()
  const path = `/tickets/${ticket.number ?? ticket.id}`

  // Write the note in one browser.
  await page.goto(path, { waitUntil: 'domcontentloaded' })
  await noteEditor(page).click()
  await page.waitForTimeout(500)
  await page.keyboard.type(NOTE)

  // Open it in a fresh browser, where the content arrives from the server.
  const fresh = await browser.newContext({ storageState: AUTH_STATE })
  try {
    const other = await fresh.newPage()
    await other.goto(path, { waitUntil: 'domcontentloaded' })
    await expect(noteEditor(other)).toContainText(NOTE, { timeout: 20_000 })

    await noteEditor(other).click()
    await other.keyboard.type(TYPED)

    // The typed text lands wherever the click put the caret; around it, the
    // note is intact.
    await expect.poll(() => withoutTyped(other)).toContain(NOTE)
    await expect(noteEditor(other)).toContainText(TYPED)
    // And everyone else sees the same: the first browser, live.
    await expect.poll(() => withoutTyped(page)).toContain(NOTE)
    await expect(noteEditor(page)).toContainText(TYPED)
  } finally {
    await fresh.close()
  }
})
