import { describe, expect, it } from 'vitest'
import { orderedUserFieldSchema } from '@nosdesk/core/services/userContactService'
import { fieldOrderOf, inFieldOrder, parseSchema, serializeSchema } from '../attributeSchema'

/** What a save and a reload do to a schema: the server's jsonb keeps no key
 *  order and its map writes keys sorted, so only `field_order` carries it. */
function throughServer(schema: Record<string, unknown>): Record<string, unknown> {
  const sort = (v: unknown): unknown => {
    if (Array.isArray(v)) return v.map(sort)
    if (v && typeof v === 'object') {
      return Object.fromEntries(
        Object.entries(v as Record<string, unknown>)
          .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
          .map(([k, val]) => [k, sort(val)]),
      )
    }
    return v
  }
  return sort(schema) as Record<string, unknown>
}

const names = (schema: Record<string, unknown>) => parseSchema(schema).defs.map((d) => d.name)

describe('field order', () => {
  it('survives a save and a reload', () => {
    const built = serializeSchema({
      defs: [
        { name: 'rego', kind: 'text', required: true },
        { name: 'fuel', kind: 'select', required: false, enumValues: ['Diesel'] },
        { name: 'axles', kind: 'number', required: false },
      ],
      extras: {},
    })
    const order = fieldOrderOf(built)
    expect(order).toEqual(['rego', 'fuel', 'axles'])

    const stored = throughServer(built)
    expect(names(stored)).toEqual(['axles', 'fuel', 'rego'])
    const reloaded = inFieldOrder(stored, order)
    expect(names(reloaded)).toEqual(['rego', 'fuel', 'axles'])
    expect(throughServer(reloaded)).toEqual(stored)
  })

  it('keeps unordered keys after the ordered ones, and all of them without an order', () => {
    const schema = { type: 'object', properties: { a: {}, b: {}, c: {} } }
    expect(fieldOrderOf(inFieldOrder(schema, ['c', 'gone']))).toEqual(['c', 'a', 'b'])
    expect(fieldOrderOf(inFieldOrder(schema, undefined))).toEqual(['a', 'b', 'c'])
  })

  it('keeps the keywords the builder does not edit through a move and an edit', () => {
    const stored = {
      type: 'object',
      properties: {
        office_location: { type: 'string', title: 'Office location', synced: true },
        gender: { type: 'string', title: 'Gender', enum: ['Male', 'Female'], 'x-allow-custom': true },
        cost_centre: { type: 'string', staffOnly: true, maxLength: 12 },
      },
    }
    const parsed = parseSchema(stored)
    // Move the last field to the top and edit its length, as the builder does.
    const [first, second, third] = parsed.defs
    const moved = serializeSchema({
      defs: [{ ...third, maxLength: 20 }, first, second],
      extras: parsed.extras,
    })
    const props = moved.properties as Record<string, Record<string, unknown>>
    expect(Object.keys(props)).toEqual(['cost_centre', 'office_location', 'gender'])
    expect(props.cost_centre).toEqual({ type: 'string', staffOnly: true, maxLength: 20 })
    expect(props.office_location).toEqual(stored.properties.office_location)
    expect(props.gender).toEqual(stored.properties.gender)
  })

  it('lays the user field schema out in its field_order and drops the key', () => {
    const fromServer = {
      type: 'object',
      properties: { birthday: { type: 'string' }, gender: { type: 'string' }, house: { type: 'string' } },
      field_order: ['house', 'birthday'],
    }
    const schema = orderedUserFieldSchema(fromServer)
    expect(Object.keys(schema.properties as object)).toEqual(['house', 'birthday', 'gender'])
    expect(schema).not.toHaveProperty('field_order')

    // A 1.1 server sends no order: the schema reads as it came.
    const bare = orderedUserFieldSchema({ type: 'object', properties: { b: {}, a: {} } })
    expect(Object.keys(bare.properties as object)).toEqual(['b', 'a'])
  })
})
