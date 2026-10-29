import { describe, expect, it } from 'vitest'
import { createPdfjsL10n } from '@/utils/pdfjsL10n'

const translate = (key: string, args?: Record<string, string | number>) =>
  args ? `${key}(${Object.values(args).join(',')})` : key

const labelled = (id: string, args?: Record<string, unknown>, tag = 'div') => {
  const element = document.createElement(tag)
  element.setAttribute('data-l10n-id', id)
  if (args) element.setAttribute('data-l10n-args', JSON.stringify(args))
  return element
}

describe('createPdfjsL10n', () => {
  it("labels pdf.js's pages and annotations from the catalogue", async () => {
    const l10n = createPdfjsL10n(translate, 'en-AU')
    const root = document.createElement('div')
    const page = labelled('pdfjs-page-landmark', { page: 3 })
    const icon = labelled('pdfjs-text-annotation-type', { type: 'Note' }, 'img')
    const comment = labelled('pdfjs-show-comment-button', undefined, 'button')
    root.append(page, icon, comment)
    await l10n.translate(root)
    expect(page.getAttribute('aria-label')).toBe('ticket-media-pdf-page-landmark(3)')
    expect(icon.getAttribute('alt')).toBe('ticket-media-pdf-annotation-type(Note)')
    expect(comment.getAttribute('title')).toBe('ticket-media-pdf-show-comment')
    await l10n.destroy()
  })

  it('labels pages as pdf.js adds them', async () => {
    const l10n = createPdfjsL10n(translate, 'en-AU')
    const root = document.createElement('div')
    await l10n.translate(root)
    const page = labelled('pdfjs-page-landmark', { page: 7 })
    root.appendChild(page)
    await new Promise((r) => setTimeout(r))
    expect(page.getAttribute('aria-label')).toBe('ticket-media-pdf-page-landmark(7)')
    await l10n.destroy()
  })

  it("formats a comment's date in the reader's locale, and leaves unknown labels alone", async () => {
    const l10n = createPdfjsL10n(translate, 'en-AU')
    const when = new Date(2026, 9, 29, 14, 5, 9).valueOf()
    const time = labelled('pdfjs-annotation-date-time-string', { dateObj: when }, 'time')
    const other = labelled('pdfjs-editor-ink-editor')
    other.textContent = 'as it was'
    await l10n.translateOnce(time)
    await l10n.translateOnce(other)
    expect(time.textContent).toBe(new Intl.DateTimeFormat('en-AU', { dateStyle: 'short', timeStyle: 'medium' }).format(when))
    expect(other.textContent).toBe('as it was')
    expect(await l10n.get('pdfjs-page-landmark', { page: 2 })).toBe('ticket-media-pdf-page-landmark(2)')
  })
})
