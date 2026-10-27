/**
 * A workspace import as the server answers it, for the specs: Acme IT, made
 * by Nosdesk 1.1.3 on Cloud, read by this server's check.
 */
import type {
  WorkspaceImportJob,
  WorkspaceImportPerson,
  WorkspaceImportPlan,
  WorkspaceImportState,
} from '@nosdesk/core/services/workspaceImportService'

const person = (over: Partial<WorkspaceImportPerson> & Pick<WorkspaceImportPerson, 'name'>): WorkspaceImportPerson => ({
  archive_uuid: `a-${over.name}`,
  target_uuid: `t-${over.name}`,
  email: `${over.name.toLowerCase().replace(/\s+/g, '.')}@acme.example`,
  archive_role: 'agent',
  staff: true,
  disabled: false,
  outcome: 'created',
  role_on_target: 'agent',
  username_dropped: false,
  uuid_reminted: false,
  ...over,
})

export const PEOPLE: WorkspaceImportPerson[] = [
  person({ name: 'Priya Natarajan', archive_role: 'owner', outcome: 'merged_into_runner', role_on_target: 'owner' }),
  person({ name: 'Tom Okafor', archive_role: 'owner', role_on_target: 'admin' }),
  person({ name: 'Mei Lin', archive_role: 'admin', role_on_target: 'admin' }),
  person({ name: 'Hannah Cole', outcome: 'mapped' }),
  person({ name: 'Olivia Grant', disabled: true, role_on_target: null }),
  person({ name: 'Ben Carter', email: null }),
  person({ name: 'Jonas Berg', username_dropped: true }),
  person({ name: 'Rita Requester', staff: false, archive_role: 'member', role_on_target: 'member' }),
  person({ name: 'Sam Requester', staff: false, archive_role: 'member', role_on_target: 'member' }),
  person({ name: 'Ana Requester', staff: false, archive_role: 'member', role_on_target: 'member', outcome: 'mapped' }),
]

const table = (name: string, planned: number, cls = 'move') => ({ table: name, class: cls, in_archive: planned, planned })

export const PLAN: WorkspaceImportPlan = {
  mode: 'self_hosted',
  tables: [
    table('tickets', 2412),
    table('comments', 9874),
    table('attachments', 1338),
    table('ticket_merges', 38),
    table('documentation_pages', 180, 'replace'),
    table('documentation_collections', 6, 'replace'),
    table('assets', 612),
    table('asset_loans', 38),
    table('projects', 9),
    table('cycles', 14),
    table('groups', 6),
    table('workflow_states', 7, 'replace'),
    table('rules', 7),
    table('canned_responses', 31),
    table('webhooks', 2),
    table('channels', 1),
    table('plugins', 2),
    table('workspace_email_settings', 1),
    table('workspace_identity_providers', 1),
    { ...table('notifications', 40, 'merge'), planned: 37, dropped: { unresolved_reference: 3 } },
    { ...table('outbound_emails', 40), planned: 28, dropped: { queued_mail: 12 } },
  ],
  people: PEOPLE,
  counts: {
    merged_into_runner: 1,
    mapped: 2,
    created: 7,
    placeholders: 0,
    requesters_unlinked: 0,
    usernames_dropped: 1,
    uuids_reminted: 0,
  },
  handover: [
    { kind: 'encrypted_setting_dropped', table: 'workspace_email_settings', column: 'encrypted_password', rows: 1 },
    { kind: 'switched_off', table: 'workspace_email_settings', column: 'enabled', rows: 1 },
    { kind: 'switched_off', table: 'workspace_identity_providers', column: 'enabled', rows: 1 },
    { kind: 'switched_off', table: 'webhooks', column: 'enabled', rows: 2 },
    { kind: 'switched_off', table: 'channels', column: 'enabled', rows: 1 },
    { kind: 'switched_off', table: 'plugins', column: 'state', rows: 2 },
    { kind: 'plugin_secrets_dropped', rows: 3 },
    { kind: 'tokens_regenerated', table: 'tickets', column: 'guest_lookup_token', rows: 2412 },
  ],
  references: [
    { table: 'rules', column: 'definition', effect: 'rule_drafted', count: 1 },
    { table: 'saved_views', column: 'filters', effect: 'predicate_dropped', count: 1 },
    { table: 'comments', column: 'content', effect: 'mention_as_text', count: 4 },
  ],
  refusals: [],
}

export function job(state: WorkspaceImportState, over: Partial<WorkspaceImportJob> = {}): WorkspaceImportJob {
  const checked = !['uploaded', 'checking'].includes(state)
  const applied = ['imported', 'failed', 'reset'].includes(state)
  return {
    id: 'wimp-1',
    workspace_id: 1,
    runner: 'owner',
    state,
    stage: state === 'importing' ? 'rows' : null,
    archive: { name: 'acme-it-2026-10-18.nosdesk', size: 684 * 1024 * 1024, sealed: false, stored: state !== 'imported' },
    regenerate_uuids: false,
    report: checked
      ? {
          source: {
            nosdesk_version: '1.1.3',
            format: 2,
            created_at: '2026-10-18T05:40:00Z',
            mode: 'hosted',
            workspace_slug: 'acme-it',
            origin: 'https://acme-it.nosdesk.com',
            custom_domain: null,
            sealed: false,
            secrets: false,
            files: 1638,
          },
          plan: PLAN,
          ...(applied ? { files: { restored: 1636, present: 0, skipped: [{ path: 'files/assets/412/media/front.jpg', reason: 'missing' }, { path: 'files/tickets/9/run.exe', reason: 'refused' }] } } : {}),
        }
      : null,
    error: null,
    people_created: applied ? 8 : 0,
    files_restored: applied ? 1638 : 0,
    created_at: '2026-10-21T09:14:00Z',
    updated_at: '2026-10-21T09:21:00Z',
    checked_at: checked ? '2026-10-21T09:15:00Z' : null,
    confirmed_at: applied || state === 'importing' ? '2026-10-21T09:16:00Z' : null,
    applied_at: applied ? '2026-10-21T09:18:00Z' : null,
    finished_at: applied ? '2026-10-21T09:22:40Z' : null,
    reset_at: state === 'reset' ? '2026-10-21T10:02:00Z' : null,
    pruned_at: null,
    actions: {
      check: state === 'uploaded' || state === 'previewed',
      confirm: state === 'previewed',
      retry_files: false,
      reset: !['checking', 'importing', 'reset'].includes(state),
      invite: state === 'imported',
    },
    ...over,
  }
}
