import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter } from 'vue-router'
import type { EditorView } from 'prosemirror-view'
import { TextSelection } from 'prosemirror-state'
import { TableMap } from 'prosemirror-tables'

// The editor reads its document from the collab session; here the session is
// a local Y.Doc with no socket, holding a table.
const collab = vi.hoisted(() => ({
  session: null as null | Record<string, unknown>,
}))
const auth = vi.hoisted(() => ({ isTechnician: true }))
const captured = vi.hoisted(() => ({ view: null as null | import('prosemirror-view').EditorView }))

// Hold on to the live view through the table controls plugin.
vi.mock('@/components/editor/tableView', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@/components/editor/tableView')>()
  const { Plugin } = await import('prosemirror-state')
  return {
    ...mod,
    createTableControlsPlugin: (onState: Parameters<typeof mod.createTableControlsPlugin>[0]) => {
      const plugin = mod.createTableControlsPlugin(onState)
      return new Plugin({
        ...plugin.spec,
        view(view) {
          captured.view = view
          return plugin.spec.view!(view)
        },
      })
    },
  }
})

vi.mock('extendable-media-recorder', () => ({ MediaRecorder: class {}, register: async () => {} }))
vi.mock('extendable-media-recorder-wav-encoder', () => ({ connect: async () => ({}) }))
vi.mock('@/stores/collabSession', async () => {
  const { reactive } = await import('vue')
  const store = reactive({
    connectionBadge: {} as Record<string, string | null>,
    connectionRefusal: {} as Record<string, string | null>,
    acquire: () => collab.session,
    release: () => {},
  })
  return { useCollabSessionStore: () => store }
})
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    isAuthenticated: true,
    get isTechnician() {
      return auth.isTechnician
    },
    user: { uuid: 'u-1', name: 'Ana' },
  }),
}))

import * as Y from 'yjs'
import { Awareness } from 'y-protocols/awareness'
import { prosemirrorJSONToYXmlFragment } from 'y-prosemirror'
import { configureTransport, type AuthStrategy } from '@nosdesk/core/transport'
import { mountWithProviders } from '@/test/mountWithProviders'
import CollaborativeEditor from '@/components/CollaborativeEditor.vue'
import { schema } from '@/components/editor/schema'
import { allowTables, resetTablesAllowed } from '@/components/editor/tablesAllowed'

const DOC_ID = 'ws-00000000-0000-0000-0000-000000000001_doc-00000000-0000-0000-0000-000000000002'

function seedSession() {
  const ydoc = new Y.Doc()
  const cell = (type: string, text: string) => ({ type, content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] })
  prosemirrorJSONToYXmlFragment(
    schema,
    {
      type: 'doc',
      content: [
        { type: 'paragraph', content: [{ type: 'text', text: 'Intro' }] },
        {
          type: 'table',
          content: [
            { type: 'table_row', content: [cell('table_header', 'Name'), cell('table_header', 'Owner')] },
            { type: 'table_row', content: [cell('table_cell', 'VPN'), cell('table_cell', 'Ana')] },
          ],
        },
        { type: 'paragraph' },
      ],
    },
    ydoc.getXmlFragment('prosemirror'),
  )
  const provider = { awareness: new Awareness(ydoc), on: () => {}, off: () => {}, ws: null }
  collab.session = { ydoc, provider, permanentUserData: { setUserMapping: () => {} }, isNew: false }
}

configureTransport({ baseUrl: '/api', collabWsBaseUrl: 'ws://localhost/collaboration/ws', auth: {} as AuthStrategy })

let wrapper: VueWrapper | null = null
beforeEach(() => {
  seedSession()
  auth.isTechnician = true
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  resetTablesAllowed()
  document.body.innerHTML = ''
})

async function mountEditor(props: Record<string, unknown> = {}) {
  document.body.innerHTML = '<div id="overlays"></div>'
  const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: { render: () => null } }] })
  wrapper = mountWithProviders(CollaborativeEditor, { docId: DOC_ID, hideRevisionHistory: true, ...props }, {}, [router])
  await flushPromises()
}

function viewOf(): EditorView {
  if (!captured.view) throw new Error('no view')
  return captured.view
}

function caretInCell(view: EditorView, row: number, col: number) {
  let tablePos = -1
  view.state.doc.forEach((node, offset) => {
    if (node.type.name === 'table') tablePos = offset
  })
  const map = TableMap.get(view.state.doc.nodeAt(tablePos)!)
  const pos = tablePos + 1 + map.map[row * map.width + col]! + 2
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, pos)))
}

const tableButton = () => document.querySelector('.toolbar-button--table')

describe('the toolbar Table button', () => {
  it('shows while the caret is in a table, where tables are allowed', async () => {
    allowTables(DOC_ID)
    await mountEditor()
    expect(document.querySelector('.pm-table')).not.toBeNull()
    expect(tableButton()).toBeNull()
    caretInCell(viewOf(), 1, 0)
    await flushPromises()
    expect(tableButton()).not.toBeNull()
    expect(tableButton()!.getAttribute('aria-haspopup')).toBe('menu')
    viewOf().dispatch(viewOf().state.tr.setSelection(TextSelection.create(viewOf().state.doc, 2)))
    await flushPromises()
    expect(tableButton()).toBeNull()
  })

  it('stays hidden, and the table controls with it, before the server allows tables', async () => {
    await mountEditor()
    caretInCell(viewOf(), 1, 0)
    await flushPromises()
    expect(tableButton()).toBeNull()
    expect(document.querySelector('.collaborative-editor')!.classList.contains('table-controls-off')).toBe(true)
    allowTables(DOC_ID)
    await flushPromises()
    expect(document.querySelector('.collaborative-editor')!.classList.contains('table-controls-off')).toBe(false)
    expect(tableButton()).not.toBeNull()
  })

  it('is absent read only, and the table renders without controls', async () => {
    allowTables(DOC_ID)
    await mountEditor({ readonly: true })
    caretInCell(viewOf(), 1, 0)
    await flushPromises()
    expect(document.querySelector('.toolbar')).toBeNull()
    expect(document.querySelector('.collaborative-editor')!.classList.contains('table-controls-off')).toBe(true)
    expect(document.querySelector('.ProseMirror')!.getAttribute('contenteditable')).toBe('false')
    expect(document.querySelector('.pm-table')!.classList.contains('is-active')).toBe(false)
  })

  it('offers Table in the Insert menu only where tables are allowed', async () => {
    await mountEditor()
    const insert = () => document.querySelector('[aria-label="editor-toolbar-insert"]') as HTMLElement
    insert().click()
    await flushPromises()
    expect(document.body.textContent).not.toContain('editor-insert-menu-table')
    insert().click()
    await flushPromises()
    allowTables(DOC_ID)
    insert().click()
    await flushPromises()
    expect(document.body.textContent).toContain('editor-insert-menu-table')
  })
})

describe('the Insert menu', () => {
  // Desktop: the menu is a Reka DropdownMenu rather than the phone sheet.
  const matchMedia = window.matchMedia
  beforeEach(() => {
    window.matchMedia = ((query: string) => ({
      ...matchMedia(query),
      matches: query.includes('min-width'),
    })) as typeof window.matchMedia
  })
  afterEach(() => {
    window.matchMedia = matchMedia
  })

  const insert = () => document.querySelector('[aria-label="editor-toolbar-insert"]') as HTMLElement
  const settle = () => new Promise((r) => setTimeout(r, 30))

  async function pick(label: string) {
    insert().click()
    await flushPromises()
    const row = Array.from(document.querySelectorAll<HTMLElement>('[role="menuitem"]')).find(
      (el) => el.textContent?.trim() === label,
    )
    if (!row) throw new Error(`no ${label} row`)
    row.focus()
    row.click()
    await flushPromises()
    await settle()
  }

  it('inserts a table with the editor focused and the caret in its first header cell', async () => {
    allowTables(DOC_ID)
    await mountEditor()
    const view = viewOf()
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 2)))
    await pick('editor-insert-menu-table')
    expect(document.querySelector('[role="menu"]')).toBeNull()
    expect(document.activeElement).toBe(view.dom)
    expect(document.activeElement).not.toBe(insert())
    const { $from } = view.state.selection
    expect($from.parent.type.name).toBe('paragraph')
    expect($from.node(-1).type.name).toBe('table_header')
    expect($from.index(-2)).toBe(0)
    expect($from.index(-3)).toBe(0)
    // Typing lands in that cell.
    view.dispatch(view.state.tr.insertText('Asset'))
    expect(view.state.selection.$from.node(-1).textContent).toBe('Asset')
  })

  it('quotes the paragraph on an empty page without throwing, and keeps the editor focused', async () => {
    await mountEditor()
    const view = viewOf()
    view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, schema.nodes.paragraph.create()))
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 1)))
    const errors: unknown[] = []
    const onError = (event: ErrorEvent) => errors.push(event.error)
    window.addEventListener('error', onError)
    await pick('editor-insert-menu-blockquote')
    window.removeEventListener('error', onError)
    expect(errors).toEqual([])
    expect(view.state.doc.firstChild!.type.name).toBe('blockquote')
    expect(document.activeElement).toBe(view.dom)
  })
})
