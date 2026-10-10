import { beforeEach, describe, expect, it, vi } from 'vitest'
import { FluentBundle, FluentResource } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'

// A change the server refuses is rolled back and the person is told why,
// once per push, whoever made the change.

const sources = import.meta.glob(['../../../i18n/locales/*/main.ftl', '../../../backend/src/handlers/sync/push.rs'], {
  eager: true,
  query: '?raw',
  import: 'default',
}) as Record<string, string>
const catalogue = (locale: string) => sources[`../../../i18n/locales/${locale}/main.ftl`]

const toast = vi.hoisted(() => ({ error: vi.fn() }))
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))
const pool = vi.hoisted(() => ({ patch: vi.fn() }))
vi.mock('@nosdesk/core/sync/pool', () => ({
  currentEpoch: () => 1,
  patch: pool.patch,
  get: vi.fn(),
  getLastSyncId: () => null,
}))
const push = vi.hoisted(() => ({ response: {} as unknown }))
vi.mock('@/services/authFetch', () => ({
  authFetch: async () => new Response(JSON.stringify(push.response), { status: 200 }),
}))
const pending = vi.hoisted(() => ({ txs: [] as Array<{ tx_id: string }> }))
vi.mock('./idb', () => ({
  loadTransactions: async () => [...pending.txs],
  deleteTransaction: async (_h: unknown, txId: string) => {
    pending.txs = pending.txs.filter((t) => t.tx_id !== txId)
  },
  putTransaction: vi.fn(),
}))

const assign = (id: number) => ({
  tx_id: `tx-${id}`,
  aggregate: 'ticket',
  model_id: String(id),
  op: 'U',
  patch: { assignee_uuid: 'u-requester', last_activity_at: '2026-10-10T00:00:00Z' },
  inverse: { assignee_uuid: null },
  base_sync_id: null,
  createdAt: 0,
})

let queue: typeof import('./queue')
beforeEach(async () => {
  vi.resetModules()
  toast.error.mockReset()
  pool.patch.mockReset()
  const bundle = new FluentBundle('en-US', { useIsolating: false })
  bundle.addResource(new FluentResource(catalogue('en-US')))
  // After the reset, so the queue reads the same instance.
  const { setActiveFluent } = await import('@nosdesk/core/i18n')
  setActiveFluent(createFluentVue({ bundles: [bundle] }))
  queue = await import('./queue')
  queue.setIdbHandle({} as never)
})

describe('a push the server refuses', () => {
  it('rolls each change back and tells the person once, with the reason', async () => {
    pending.txs = [assign(10), assign(11)]
    const refused = 'Only agents and admins can be assigned tickets'
    push.response = {
      applied: [],
      rejected: [
        { tx_id: 'tx-10', reason: 'invalid_assignee', detail: refused },
        { tx_id: 'tx-11', reason: 'invalid_assignee', detail: refused },
      ],
      last_sync_id: 0,
    }

    await queue.flush()

    expect(pool.patch.mock.calls).toEqual([
      ['ticket', '10', { assignee_uuid: null }],
      ['ticket', '11', { assignee_uuid: null }],
    ])
    expect(pending.txs).toEqual([])
    expect(toast.error.mock.calls).toEqual([
      ["Couldn't assign 2 tickets", 'Only agents and admins can be assigned tickets.'],
    ])
  })

  it('says nothing when every change was applied', async () => {
    pending.txs = [assign(10)]
    push.response = { applied: ['tx-10'], rejected: [], last_sync_id: 5 }

    await queue.flush()

    expect(pool.patch).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })
})

describe('the rejection messages', () => {
  // Every reason push.rs can send, read from the source so a new one fails
  // here until it has a message.
  const pushSource = sources['../../../backend/src/handlers/sync/push.rs']
  const serverReasons = [...new Set([...pushSource.matchAll(/TxReject\(\s*"([a-z_]+)"/g)].map((m) => m[1]))]
  const keysIn = (locale: string) => new Set([...catalogue(locale).matchAll(/^([a-z0-9-]+) =/gm)].map((m) => m[1]))

  it('cover every reason the server sends', async () => {
    const { REASON_MESSAGES } = await import('./rejections')
    expect(serverReasons.length).toBeGreaterThanOrEqual(13)
    expect(serverReasons.filter((r) => !(r in REASON_MESSAGES))).toEqual([])
  })

  it('exist in en-US, fr-FR and nl-NL', async () => {
    const { REASON_MESSAGES, GENERIC_REASON } = await import('./rejections')
    const titles = [
      'sync-rejected-assign',
      'sync-rejected-status',
      'sync-rejected-priority',
      'sync-rejected-rename',
      'sync-rejected-tags',
      'sync-rejected-dates',
      'sync-rejected-ticket',
      'sync-rejected-project',
      'sync-rejected-other',
    ]
    const needed = [...new Set([...Object.values(REASON_MESSAGES), GENERIC_REASON, ...titles])]
    for (const locale of ['en-US', 'fr-FR', 'nl-NL']) {
      const keys = keysIn(locale)
      expect(needed.filter((k) => !keys.has(k)), locale).toEqual([])
    }
  })

  it('map each reason, with a generic message for one it does not know', async () => {
    const { reasonMessageKey } = await import('./rejections')
    expect(reasonMessageKey('invalid_assignee')).toBe('sync-rejected-reason-invalid-assignee')
    expect(reasonMessageKey('approval_pending')).toBe('sync-rejected-reason-approval-pending')
    expect(reasonMessageKey('ticket_merged')).toBe('sync-rejected-reason-ticket-merged')
    expect(reasonMessageKey('invalid_patch')).toBe('sync-rejected-reason-unsupported')
    expect(reasonMessageKey('something_new')).toBe('sync-rejected-reason-generic')
    expect(reasonMessageKey('toString')).toBe('sync-rejected-reason-generic')
  })

  it('name the change from the fields it set', async () => {
    const { actionTitleKey } = await import('./rejections')
    expect(actionTitleKey('ticket', { workflow_state_id: 3, workflow_state: {}, last_activity_at: 'x' })).toBe(
      'sync-rejected-status',
    )
    expect(actionTitleKey('ticket', { due_date: null, start_date: null })).toBe('sync-rejected-dates')
    expect(actionTitleKey('ticket', { priority: 'high', assignee_uuid: 'u' })).toBe('sync-rejected-ticket')
    expect(actionTitleKey('ticket', { category_id: 4 })).toBe('sync-rejected-ticket')
    expect(actionTitleKey('project', { name: 'x' })).toBe('sync-rejected-project')
  })

  it('group a mixed push by change and reason', async () => {
    const { summariseRejections } = await import('./rejections')
    expect(
      summariseRejections([
        { aggregate: 'ticket', patch: { priority: 'high' }, reason: 'ticket_merged' },
        { aggregate: 'ticket', patch: { workflow_state_id: 2 }, reason: 'approval_pending' },
        { aggregate: 'ticket', patch: { priority: 'low' }, reason: 'ticket_merged' },
        { aggregate: 'ticket', patch: { title: 'x' }, reason: 'brand_new' },
      ]),
    ).toEqual([
      { title: "Couldn't change the priority of 2 tickets", message: "Merged tickets can't be changed." },
      {
        title: "Couldn't change the status of 1 ticket",
        message: 'Requests waiting for approval stay open. Approve or skip the approval first.',
      },
      { title: "Couldn't rename 1 ticket", message: "The server didn't accept the change." },
    ])
  })
})
