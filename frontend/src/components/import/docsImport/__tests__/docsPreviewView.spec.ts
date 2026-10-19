import { afterEach, describe, expect, it, vi } from 'vitest'

import { schema } from '@/components/editor/schema'

import { buildDoc, mountPreview, type PreviewHandle, type PreviewLabels } from '../docsPreviewView'

const labels: PreviewLabels = {
  callout: 'Was a callout',
  checklist: 'Was a checklist',
  tableMoved: 'Moved out of the list',
  image: (info, alt) => (info?.problem ? `problem ${info.problem}` : `${info?.size ?? ''} ${alt ?? ''}`.trim()),
  imageAria: (_info, name) => `Image: ${name}`,
}

const text = (t: string, marks?: object[]) => (marks ? { type: 'text', text: t, marks } : { type: 'text', text: t })
const p = (...content: object[]) => ({ type: 'paragraph', content })
const cell = (type: string, t: string) => ({ type, attrs: { colspan: 1, rowspan: 1, colwidth: null, align: null }, content: [p(text(t))] })

const DOC = {
  type: 'doc',
  content: [
    p(text('See '), text('VPN', [{ type: 'link', attrs: { href: 'nosdesk-import:page/4', title: null } }])),
    { type: 'blockquote', content: [p(text('Tip', [{ type: 'strong' }])), p(text('Rotate the key.'))] },
    { type: 'bullet_list', content: [{ type: 'list_item', content: [p(text('☐ Buddy assigned'))] }] },
    p({ type: 'image', attrs: { src: 'nosdesk-import:preview-image/0', alt: 'diagram', title: null, width: null, align: null } }),
    { type: 'table', content: [{ type: 'table_row', content: [cell('table_header', 'Id')] }, { type: 'table_row', content: [cell('table_cell', '50')] }] },
  ],
}

let handle: PreviewHandle | null = null
afterEach(() => {
  handle?.destroy()
  handle = null
})

describe('the preview over the editor schema', () => {
  it('builds a valid page and refuses one the schema rejects', () => {
    expect(buildDoc(schema, DOC).childCount).toBe(5)
    // A table inside a list is not valid in the editor.
    const bad = { type: 'doc', content: [{ type: 'bullet_list', content: [{ type: 'list_item', content: [DOC.content[4]] }] }] }
    expect(() => buildDoc(schema, bad)).toThrow()
    expect(() => buildDoc(schema, { type: 'doc', content: [{ type: 'nonsense' }] })).toThrow()
  })

  it('is read only, shows images as named placeholders and labels converted spots', () => {
    const mount = document.createElement('div')
    handle = mountPreview({
      mount,
      schema,
      json: DOC,
      images: { 'nosdesk-import:preview-image/0': { name: 'vpn.png', size: 240, kind: 'archive', problem: 'unreadable' } },
      notes: { callouts_as_quotes: 1, checklists_as_bullets: 1, tables_hoisted: 1 },
      labels,
      ariaLabel: 'Preview of VPN',
    })
    expect(handle.view.editable).toBe(false)
    expect(mount.querySelector('img:not(.ProseMirror-separator)')).toBeNull()
    const placeholder = mount.querySelector('.docs-preview-img')!
    expect(placeholder.getAttribute('aria-label')).toBe('Image: vpn.png')
    expect(placeholder.classList.contains('is-problem')).toBe(true)
    expect(placeholder.textContent).toContain('problem unreadable')
    const notes = [...mount.querySelectorAll('.docs-preview-note')].map((el) => el.getAttribute('data-note'))
    expect(notes).toEqual(['Was a callout', 'Was a checklist', 'Moved out of the list'])
  })

  it('labels nothing the page has no note for', () => {
    const mount = document.createElement('div')
    handle = mountPreview({ mount, schema, json: DOC, images: {}, notes: {}, labels, ariaLabel: 'x' })
    expect(mount.querySelectorAll('.docs-preview-note')).toHaveLength(0)
  })

  it('follows a link to another page in the import instead of navigating', () => {
    const mount = document.createElement('div')
    document.body.appendChild(mount)
    const onPageLink = vi.fn()
    handle = mountPreview({ mount, schema, json: DOC, images: {}, notes: {}, labels, ariaLabel: 'x', onPageLink })
    const link = mount.querySelector('a')!
    const event = new MouseEvent('click', { bubbles: true, cancelable: true })
    link.dispatchEvent(event)
    expect(onPageLink).toHaveBeenCalledWith(4)
    expect(event.defaultPrevented).toBe(true)
    mount.remove()
  })
})
