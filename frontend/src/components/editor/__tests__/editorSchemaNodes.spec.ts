/**
 * The collaboration server keeps a table of the first editor schema version
 * that knows each node type, and refuses an older client on a document that
 * holds one. Cargo reads this fixture and fails when a node here is missing
 * from that table, so a new node type can't ship without its gate entry.
 *
 * Regenerate after changing the schema's nodes:
 *   UPDATE_EDITOR_SCHEMA_NODES=1 pnpm vitest run editorSchemaNodes
 */
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import { schema } from '../schema'

const FIXTURE = resolve(__dirname, '../../../../../backend/tests/fixtures/editor_schema_nodes.json')

describe('editor_schema_nodes.json', () => {
  it("lists the documentation schema's node types", () => {
    const names = Object.keys(schema.nodes).sort()
    if (process.env.UPDATE_EDITOR_SCHEMA_NODES) {
      writeFileSync(FIXTURE, `${JSON.stringify(names, null, 2)}\n`)
    }
    expect(existsSync(FIXTURE), `${FIXTURE} is missing; regenerate it (see this file)`).toBe(true)
    const listed = JSON.parse(readFileSync(FIXTURE, 'utf8'))
    expect(listed, 'stale fixture; regenerate it (see this file)').toEqual(names)
  })
})
