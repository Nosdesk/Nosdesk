import { test, expect, type Page } from '@playwright/test'
import { AUTH_STATE } from './helpers'

/**
 * Typing into a documentation page just opened keeps what it already says.
 *
 * The page's content reaches its editor after the editor mounts: from the
 * server in a fresh browser, or from IndexedDB on a reload. A click into the
 * middle of a paragraph and a few keystrokes add to the page; they must not
 * replace it with what was typed, for every viewer and in what is saved.
 */
test.skip(({ hasTouch }) => hasTouch, 'pointer input')

const PARAGRAPHS = ['First paragraph.', 'Second paragraph.', 'Third paragraph.']
/** What each step types. Each is taken out again, latest first, to check the
 *  page around them is intact wherever the caret put them. */
const TYPED = [' (clicked cold)', ' (clicked warm)']

const editor = (page: Page) => page.locator('.ProseMirror:not(.simple-editor-content)').first()

/** The page's text, without other people's cursor labels and without what
 *  the test typed. */
const untyped = (page: Page) =>
  editor(page).evaluate((el, typed) => {
    const copy = el.cloneNode(true) as HTMLElement
    copy.querySelectorAll('.ProseMirror-yjs-cursor').forEach((c) => c.remove())
    let text = copy.textContent ?? ''
    for (const t of [...typed].reverse()) text = text.replace(t, '')
    return text
  }, TYPED)

/** Click inside the text of the paragraph that starts with `prefix`, a few
 *  characters in, the way a reader clicks into a sentence. */
async function clickInto(page: Page, prefix: string) {
  const at = await editor(page).evaluate((el, prefix) => {
    const p = [...el.querySelectorAll('p')].find((n) => n.textContent?.startsWith(prefix))
    const text = p?.firstChild
    if (!text || text.nodeType !== Node.TEXT_NODE) return null
    const range = document.createRange()
    range.setStart(text, 3)
    range.setEnd(text, 4)
    const r = range.getBoundingClientRect()
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
  }, prefix)
  expect(at, `paragraph starting "${prefix}"`).not.toBeNull()
  await page.mouse.click(at!.x, at!.y)
}

async function csrfHeaders(page: Page): Promise<Record<string, string>> {
  const csrf = (await page.context().cookies()).find((c) => c.name === 'csrf_token')?.value ?? ''
  return { 'X-CSRF-Token': csrf }
}

test('typing into a freshly opened documentation page keeps its text', async ({ page, browser }) => {
  const headers = await csrfHeaders(page)
  const created = await page.request.post('/api/documentation/pages', {
    headers,
    data: { title: 'E2E doc first keystroke', icon: '📄', status: 'published', parent_id: null },
  })
  expect(created.ok()).toBe(true)
  const doc = (await created.json()) as { id: number; slug: string }
  const path = `/documentation/${doc.slug}`
  const fresh = await browser.newContext({ storageState: AUTH_STATE })
  const check = await browser.newContext({ storageState: AUTH_STATE })
  try {
    // Write the page in one browser.
    await page.goto(path, { waitUntil: 'domcontentloaded' })
    await editor(page).click()
    await expect(editor(page)).toBeFocused()
    await page.keyboard.type(PARAGRAPHS[0])
    for (const p of PARAGRAPHS.slice(1)) {
      await page.keyboard.press('Enter')
      await page.keyboard.type(p)
    }
    await expect(editor(page).locator('p')).toHaveText(PARAGRAPHS)

    // A fresh browser: the content arrives from the server. Click into the
    // second paragraph and type.
    const other = await fresh.newPage()
    await other.goto(path, { waitUntil: 'domcontentloaded' })
    await expect(editor(other)).toContainText(PARAGRAPHS[2], { timeout: 20_000 })
    await clickInto(other, 'Second')
    await other.keyboard.type(TYPED[0])
    await expect(editor(other)).toContainText(TYPED[0])
    await expect.poll(() => untyped(other)).toBe(PARAGRAPHS.join(''))

    // Reloaded in the same browser: the content now also comes from its cache.
    await other.reload({ waitUntil: 'domcontentloaded' })
    await expect(editor(other)).toContainText(TYPED[0], { timeout: 20_000 })
    await clickInto(other, 'Third')
    await other.keyboard.type(TYPED[1])
    await expect(editor(other)).toContainText(TYPED[1])
    await expect.poll(() => untyped(other)).toBe(PARAGRAPHS.join(''))

    // The first browser saw it all, live.
    for (const typed of TYPED) await expect(editor(page)).toContainText(typed)
    await expect.poll(() => untyped(page)).toBe(PARAGRAPHS.join(''))

    // And it was saved: a third browser, with nothing cached, loads the same.
    const saved = await check.newPage()
    await saved.goto(path, { waitUntil: 'domcontentloaded' })
    for (const typed of TYPED) {
      await expect(editor(saved)).toContainText(typed, { timeout: 20_000 })
    }
    await expect.poll(() => untyped(saved)).toBe(PARAGRAPHS.join(''))
  } finally {
    await fresh.close()
    await check.close()
    await page.request.delete(`/api/documentation/pages/${doc.id}`, { headers })
  }
})
