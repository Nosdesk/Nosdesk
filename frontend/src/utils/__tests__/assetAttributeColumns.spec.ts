import { describe, expect, it } from 'vitest'
import type { AssetKind } from '@nosdesk/core/services/assetKindsService'
import { attributeColumnDefs } from '@/utils/assetAttributeColumns'

function kind(id: number, sort_order: number, properties: Record<string, unknown>): AssetKind {
  return {
    id,
    slug: `k${id}`,
    label: `Kind ${id}`,
    description: null,
    icon: null,
    attribute_schema: { type: 'object', properties },
    sort_order,
    is_builtin: false,
    category: 'generic',
    created_at: '',
    updated_at: '',
    created_by: null,
  }
}

describe('attributeColumnDefs', () => {
  it('makes one typed column per attribute key across kinds', () => {
    const defs = attributeColumnDefs(
      [
        kind(2, 20, { bay: { type: 'string' }, owner: { type: 'string', format: 'user-uuid' } }),
        kind(1, 10, {
          bay: { type: 'integer', title: 'Bay number' },
          serviced_on: { type: 'string', format: 'date' },
          parent: { type: 'string', format: 'asset-ref' },
          tags: { type: 'array', items: { type: 'string' } },
          sealed: { type: 'boolean' },
          hostname: { type: 'string' },
        }),
      ],
      new Set(['hostname']),
    )
    expect(defs.map((d) => [d.field, d.label, d.valueType])).toEqual([
      // The first kind by sort order wins for a shared key.
      ['attr:bay', 'Bay number', 'number'],
      ['attr:owner', 'Owner', 'user'],
      ['attr:parent', 'Parent', 'asset'],
      ['attr:sealed', 'Sealed', 'boolean'],
      ['attr:serviced_on', 'Serviced on', 'date'],
      ['attr:tags', 'Tags', 'list'],
    ])
  })
})
