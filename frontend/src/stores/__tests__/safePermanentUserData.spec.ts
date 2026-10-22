/**
 * Opening a collaborative document must not change it: the user mapping is
 * written with the client's first edit, so a view sends the server nothing
 * to save or record as a revision.
 */
import { describe, expect, it } from 'vitest'
import * as Y from 'yjs'
import { SafePermanentUserData } from '@nosdesk/core/utils/safePermanentUserData'

const USER = '0190a3c4-0000-7000-8000-000000000001'

/** A document as the server holds it, and a client that has synced it. */
function openedDocument() {
  const server = new Y.Doc()
  const paragraph = new Y.XmlElement('paragraph')
  paragraph.insert(0, [new Y.XmlText('Imported')])
  server.getXmlFragment('prosemirror').insert(0, [paragraph])

  const client = new Y.Doc()
  const pud = new SafePermanentUserData(client)
  pud.setUserMappingOnFirstEdit(client, client.clientID, USER)
  Y.applyUpdate(client, Y.encodeStateAsUpdate(server), 'provider')
  return { server, client, pud }
}

/** Whether what the client would send the server changes the server's copy. */
function changesServer(server: Y.Doc, client: Y.Doc): boolean {
  const copy = new Y.Doc()
  Y.applyUpdate(copy, Y.encodeStateAsUpdate(server))
  let changed = false
  copy.on('update', () => (changed = true))
  Y.applyUpdate(copy, Y.encodeStateAsUpdate(client, Y.encodeStateVector(copy)))
  return changed
}

describe('SafePermanentUserData.setUserMappingOnFirstEdit', () => {
  it('writes nothing while the document is only open', () => {
    const { server, client } = openedDocument()
    // A selection change runs an empty local transaction.
    client.transact(() => {}, 'ySync')
    expect(changesServer(server, client)).toBe(false)
    expect(client.getMap('users').size).toBe(0)
  })

  it('maps the client to its user with the first edit', () => {
    const { server, client, pud } = openedDocument()
    const text = (client.getXmlFragment('prosemirror').get(0) as Y.XmlElement).get(0) as Y.XmlText
    text.insert(8, ' and edited')
    expect(pud.getUserByClientId(client.clientID)).toBe(USER)
    expect(changesServer(server, client)).toBe(true)
  })

  it('records the deletions of the first edit for its user', () => {
    const { client, pud } = openedDocument()
    const text = (client.getXmlFragment('prosemirror').get(0) as Y.XmlElement).get(0) as Y.XmlText
    text.delete(0, 3)
    const deleted = text._start!.id
    expect(pud.getUserByDeletedId({ client: deleted.client, clock: deleted.clock } as never)).toBe(USER)
  })
})
