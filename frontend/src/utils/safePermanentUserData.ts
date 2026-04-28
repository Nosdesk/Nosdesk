/**
 * Wrapper around `Y.PermanentUserData` that provides fallback
 * values for missing users instead of returning null.
 *
 * `Y.PermanentUserData` itself has no `destroy()` method and
 * registers cumulative observers on the doc on every
 * construction (yjs/yjs `PermanentUserData.js`). Treat it as
 * **once per Y.Doc lifetime**, the `useCollabSessionStore`
 * constructs it during `acquire(isNew=true)` and exposes the
 * shared instance to consumers.
 */
import * as Y from 'yjs'

export class SafePermanentUserData {
  private pud: Y.PermanentUserData

  constructor(doc: Y.Doc) {
    this.pud = new Y.PermanentUserData(doc)
  }

  setUserMapping(doc: Y.Doc, clientId: number, userId: string): void {
    // Note: Y.PermanentUserData appends to a YArray with no
    // dedup. Callers must not invoke this more than once per
    // (clientId, userId) pair, see the `acquire(isNew=true)`
    // gate in useCollabSessionStore.
    this.pud.setUserMapping(doc, clientId, userId)
  }

  getUserByClientId(clientId: number): unknown {
    const user = this.pud.getUserByClientId(clientId)
    if (user === null || user === undefined) {
      return `User-${clientId}`
    }
    return user
  }

  getUserByDeletedId(id: { visibleUsers: Set<unknown>; visibleDs: unknown }): unknown {
    // y-prosemirror passes a struct-like object with `visibleUsers`
    // / `visibleDs`; Y.PermanentUserData's published types expect
    // `Y.ID`, but at runtime accept the y-prosemirror shape (the
    // internal implementation only reads a subset). Keep the cast
    // local to this one boundary so consumers stay typed.
    const user = this.pud.getUserByDeletedId(id as unknown as Parameters<typeof this.pud.getUserByDeletedId>[0])
    if (user === null || user === undefined) {
      return 'Unknown User'
    }
    return user
  }

  /** Exposed because y-prosemirror's `ySyncPlugin` reads `dss`
   *  from the permanentUserData option. */
  get dss(): unknown {
    return this.pud.dss
  }
}
