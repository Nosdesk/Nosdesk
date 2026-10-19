/**
 * Keeps tables out of a collaborative document until its server allows them.
 *
 * A server that predates tables still serves clients that delete any node
 * they can't build, so this client must not create one there. Every way a
 * table could enter (an insert command, a shortcut, paste, drop, a revision
 * restore, another plugin's appended transaction) ends in a transaction, so
 * one `filterTransaction` covers them all, including ones added later.
 * Remote changes from y-sync always pass: what the server sent is already in
 * the shared document.
 *
 * A pasted or dropped table arrives as text instead of being rejected whole.
 */
import { Plugin, PluginKey } from 'prosemirror-state'
import type { Transaction } from 'prosemirror-state'
import { Slice } from 'prosemirror-model'
import { ySyncPluginKey } from 'y-prosemirror'
import { tablesAllowed } from './tablesAllowed'
import { fragmentHasTable, tablesToTextSlice } from './tableText'

export const tableGuardKey = new PluginKey('tableGuard')

/** Whether `tr` inserts a table, row or cell (any step carrying content). */
export function transactionAddsTable(tr: Transaction): boolean {
  return tr.steps.some((step) => {
    const slice = (step as { slice?: unknown }).slice
    return slice instanceof Slice && fragmentHasTable(slice.content)
  })
}

function isRemote(tr: Transaction): boolean {
  return (tr.getMeta(ySyncPluginKey) as { isChangeOrigin?: boolean } | undefined)?.isChangeOrigin === true
}

/** The guard for the document `docId()` names. */
export function createTableGuardPlugin(docId: () => string): Plugin {
  return new Plugin({
    key: tableGuardKey,
    filterTransaction(tr) {
      if (!tr.docChanged || isRemote(tr) || tablesAllowed(docId())) return true
      return !transactionAddsTable(tr)
    },
    props: {
      transformPasted(slice) {
        return tablesAllowed(docId()) ? slice : tablesToTextSlice(slice)
      },
    },
  })
}
