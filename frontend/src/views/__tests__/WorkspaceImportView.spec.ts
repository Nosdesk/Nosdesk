import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { PiniaColada } from '@pinia/colada'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { createMemoryHistory, createRouter, type Router } from 'vue-router'

import { mountWithProviders } from '@/test/mountWithProviders'
import { job } from '@/components/import/workspaceImport/__tests__/fixtures'

const api = vi.hoisted(() => ({
  list: vi.fn(),
  latest: vi.fn(),
  get: vi.fn(),
  upload: vi.fn(),
  check: vi.fn(),
  confirm: vi.fn(),
  retryFiles: vi.fn(),
  reset: vi.fn(),
  invite: vi.fn(),
}))

vi.mock('@nosdesk/core/services/workspaceImportService', async (importOriginal) => {
  const real = await importOriginal<typeof import('@nosdesk/core/services/workspaceImportService')>()
  return {
    ...real,
    workspaceImportService: Object.fromEntries(
      Object.keys(api).map((k) => [k, (...a: unknown[]) => api[k as keyof typeof api](...a)]),
    ),
  }
})

const env = vi.hoisted(() => ({ hosted: false, owner: true }))
vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => {
  const real = await importOriginal<typeof import('@nosdesk/core/services/instanceConfig')>()
  return {
    ...real,
    isHostedDeploymentRef: {
      get value() {
        return env.hosted
      },
    },
    getServerVersion: () => '1.1.3',
  }
})
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => ({
    get isOwner() {
      return env.owner
    },
  }),
}))
vi.mock('@/components/import/workspaceImport/useWorkspaceName', async () => {
  const { computed } = await import('vue')
  return { useWorkspaceName: () => computed(() => 'Acme Helpdesk') }
})

vi.mock('@/components/common/ConfirmModal.vue', () => ({
  default: {
    props: ['show', 'title', 'confirmLabel', 'confirmDisabled'],
    emits: ['confirm', 'close'],
    template: `<div v-if="show" data-test="confirm"><p data-test="confirm-title">{{ title }}</p><slot name="body" /><button type="button" data-test="confirm-ok" :disabled="confirmDisabled" @click="$emit('confirm')">{{ confirmLabel }}</button></div>`,
  },
}))

import WorkspaceImportView from '@/views/WorkspaceImportView.vue'

let wrapper: VueWrapper | null = null
let router: Router

async function mountAt(path: string) {
  const view = { template: '<div />' }
  router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/admin/data-import/workspace', name: 'admin-workspace-import', component: view },
      { path: '/admin/data-import/workspace/:id', name: 'admin-workspace-import-detail', component: view },
      { path: '/admin/data-import', name: 'admin-data-import', component: view },
      { path: '/', name: 'home', component: view },
      { path: '/admin/:rest(.*)', component: view, name: 'admin-any' },
    ],
  })
  for (const name of ['admin-email-delivery', 'admin-channels', 'admin-auth-providers', 'admin-webhooks', 'admin-plugins', 'admin-rules', 'admin-api-tokens', 'admin-ldap', 'admin-widget']) {
    router.addRoute({ path: `/admin/x/${name}`, name, component: view })
  }
  await router.push(path)
  await router.isReady()
  wrapper = mountWithProviders(WorkspaceImportView, {}, {}, [[PiniaColada, {}] as never, router])
  await flushPromises()
}

const find = (sel: string) => wrapper!.find(`[data-test="${sel}"]`)
const checkStates = () =>
  wrapper!.findAll('[data-test="workspace-checks"] li').map((li) => li.attributes('data-state'))

beforeEach(() => {
  env.hosted = false
  env.owner = true
  api.latest.mockResolvedValue(null)
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.useRealTimers()
  vi.clearAllMocks()
})

describe('WorkspaceImportView: who sees it', () => {
  it('on Cloud, says Nosdesk support runs moves in, with the support address', async () => {
    env.hosted = true
    await mountAt('/admin/data-import/workspace')
    expect(find('workspace-import-cloud').text()).toContain('workspace-import-cloud-elsewhere')
    expect(find('workspace-import-cloud').find('a[href^="mailto:"]').exists()).toBe(true)
    expect(find('workspace-archive').exists()).toBe(false)
    expect(api.latest).not.toHaveBeenCalled()
  })

  it('tells an admin who isn\'t the owner that the owner runs it', async () => {
    env.owner = false
    await mountAt('/admin/data-import/workspace')
    expect(find('workspace-import-owner-only').exists()).toBe(true)
  })
})

describe('WorkspaceImportView: Archive', () => {
  it('uploads the file with its password, follows the check and opens Preview', async () => {
    vi.useFakeTimers()
    await mountAt('/admin/data-import/workspace')
    expect(checkStates()).toEqual(['todo', 'todo', 'todo'])
    await find('workspace-password').setValue('s3cret')
    api.upload.mockResolvedValue(job('checking'))
    api.get.mockResolvedValue(job('previewed'))
    const file = new File(['NODB'], 'acme-it.nosdesk')
    await find('workspace-drop').trigger('drop', { dataTransfer: { files: [file] } })
    await flushPromises()
    expect(api.upload).toHaveBeenCalledWith(file, { password: 's3cret' }, expect.any(Function))
    expect(router.currentRoute.value.query.job).toBe('wimp-1')
    expect(checkStates()).toEqual(['wait', 'ok', 'wait'])

    await vi.advanceTimersByTimeAsync(1600)
    await flushPromises()
    expect(find('workspace-preview').exists()).toBe(true)
  })

  it('refuses a file over the cap before uploading', async () => {
    await mountAt('/admin/data-import/workspace')
    const big = new File(['x'], 'big.nosdesk')
    Object.defineProperty(big, 'size', { value: 2 * 1024 ** 3 })
    await find('workspace-drop').trigger('drop', { dataTransfer: { files: [big] } })
    await flushPromises()
    expect(api.upload).not.toHaveBeenCalled()
    expect(checkStates()[1]).toBe('bad')
    expect(find('workspace-check-fix').attributes('data-fix')).toBe('choose_another')
  })

  it('asks a sealed archive for its password, then checks it', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValueOnce(
      job('uploaded', { archive: { name: 'a.nosdesk', size: 10, sealed: true, stored: true }, error: { code: 'workspace_import.password_required', message: '' } }),
    )
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    expect(find('workspace-check-fix').attributes('data-fix')).toBe('password')
    await find('workspace-fix-password').setValue('pw')
    api.check.mockResolvedValue(job('checking'))
    api.get.mockResolvedValue(job('previewed'))
    await find('workspace-fix-check').trigger('click')
    await flushPromises()
    expect(api.check).toHaveBeenCalledWith('wimp-1', { password: 'pw', regenerate_uuids: undefined })
    await vi.advanceTimersByTimeAsync(1600)
    await flushPromises()
    expect(find('workspace-preview').exists()).toBe(true)
  })

  it('shows what the workspace holds when it isn\'t empty, and disables the file picker', async () => {
    api.upload.mockResolvedValue(
      job('uploaded', { error: { code: 'workspace_not_empty', message: '', args: { found: [{ what: 'tickets', rows: 3 }, { what: 'files', rows: 4 }] } } }),
    )
    await mountAt('/admin/data-import/workspace')
    await find('workspace-drop').trigger('drop', { dataTransfer: { files: [new File(['x'], 'a.nosdesk')] } })
    await flushPromises()
    expect(checkStates()[0]).toBe('bad')
    expect(find('workspace-check-found').text()).toContain('workspace-import-n-tickets')
    expect(find('workspace-check-found').text()).toContain('workspace-import-found-files')
    expect(find('workspace-to-preview').attributes('disabled')).toBeDefined()
  })

  it('follows the import already under way', async () => {
    api.upload.mockRejectedValue({ response: { status: 409, data: { code: 'import_in_progress', error: 'x', args: { job_id: 'live-1', state: 'importing' } } } })
    await mountAt('/admin/data-import/workspace')
    await find('workspace-drop').trigger('drop', { dataTransfer: { files: [new File(['x'], 'a.nosdesk')] } })
    await flushPromises()
    expect(find('workspace-check-fix').attributes('data-fix')).toBe('follow')
    api.get.mockResolvedValue(job('imported', { id: 'live-1' }))
    await find('workspace-fix-follow').trigger('click')
    await flushPromises()
    expect(api.get).toHaveBeenCalledWith('live-1')
    expect(find('workspace-done').exists()).toBe(true)
  })

  it('checks a colliding archive again with new ids', async () => {
    api.get.mockResolvedValueOnce(
      job('uploaded', {
        error: {
          code: 'workspace_import.collision',
          message: '',
          args: { refusals: [{ code: 'workspace_import.collision', args: { table: 'documentation_pages', columns: ['uuid'], rows: 12 } }] },
        },
      }),
    )
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    api.check.mockResolvedValue(job('previewed'))
    await find('workspace-fix-new-ids').trigger('click')
    await flushPromises()
    expect(api.check).toHaveBeenCalledWith('wimp-1', { password: undefined, regenerate_uuids: true })
    expect(find('workspace-preview').exists()).toBe(true)
  })

  it('resumes the live import when no job is named', async () => {
    api.latest.mockResolvedValue(job('previewed'))
    await mountAt('/admin/data-import/workspace')
    expect(find('workspace-preview').exists()).toBe(true)
    expect(router.currentRoute.value.query.job).toBe('wimp-1')
  })

  it('starts fresh when the latest import is finished', async () => {
    api.latest.mockResolvedValue(job('imported'))
    await mountAt('/admin/data-import/workspace')
    expect(find('workspace-archive').exists()).toBe(true)
    expect(router.currentRoute.value.query.job).toBeUndefined()
  })
})

describe('WorkspaceImportView: Preview and confirm', () => {
  it('maps the report into its sections', async () => {
    api.get.mockResolvedValue(job('previewed'))
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    expect(wrapper!.findAll('[data-test="workspace-preview-areas"] li').map((li) => li.attributes('data-area'))).toEqual([
      'tickets', 'docs', 'assets', 'projects', 'people', 'config', 'integrations',
    ])
    expect(wrapper!.findAll('[data-test="workspace-person"]')).toHaveLength(7)
    await wrapper!.find('[data-filter="attention"]').trigger('click')
    expect(wrapper!.findAll('[data-test="workspace-person"]')).toHaveLength(6)
    expect(wrapper!.findAll('[data-test="workspace-off"] li').map((li) => li.attributes('data-off'))).toEqual([
      'workspace_email_settings', 'workspace_identity_providers', 'webhooks', 'channels', 'plugins', 'tokens',
    ])
    expect(wrapper!.findAll('[data-test="workspace-attention"] li')).toHaveLength(5)
  })

  it('imports only once the workspace name is typed, then follows the stages to Done', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValueOnce(job('previewed'))
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    await find('workspace-import-open-confirm').trigger('click')
    expect(find('confirm-ok').attributes('disabled')).toBeDefined()
    await find('workspace-confirm-name').setValue('acme')
    expect(find('confirm-ok').attributes('disabled')).toBeDefined()
    await find('workspace-confirm-name').setValue(' acme helpdesk ')
    expect(find('confirm-ok').attributes('disabled')).toBeUndefined()

    api.confirm.mockResolvedValue(job('importing', { stage: 'rows' }))
    await find('confirm-ok').trigger('click')
    await flushPromises()
    expect(api.confirm).toHaveBeenCalledWith('wimp-1', { password: undefined })
    const stage = () => wrapper!.find('[data-test="workspace-progress"] [data-state="wait"]').attributes('data-stage')
    expect(stage()).toBe('rows')

    api.get.mockResolvedValueOnce(job('importing', { stage: 'files' }))
    await vi.advanceTimersByTimeAsync(2100)
    await flushPromises()
    expect(stage()).toBe('files')

    api.get.mockResolvedValue(job('imported', { sign_in: { pending: 4, invited: 0, email_ready: true } }))
    await vi.advanceTimersByTimeAsync(2100)
    await flushPromises()
    expect(find('workspace-done-complete').exists()).toBe(true)
  })

  it('goes back to Archive when the workspace changed under the import', async () => {
    vi.useFakeTimers()
    api.get.mockResolvedValueOnce(job('previewed'))
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    await find('workspace-import-open-confirm').trigger('click')
    await find('workspace-confirm-name').setValue('Acme Helpdesk')
    api.confirm.mockResolvedValue(job('importing'))
    await find('confirm-ok').trigger('click')
    await flushPromises()
    api.get.mockResolvedValue(job('uploaded', { error: { code: 'workspace_not_empty', message: '', args: { found: [{ what: 'tickets', rows: 1 }] } } }))
    await vi.advanceTimersByTimeAsync(2100)
    await flushPromises()
    expect(find('workspace-archive').exists()).toBe(true)
    expect(checkStates()[0]).toBe('bad')
  })
})

describe('WorkspaceImportView: Done', () => {
  it('sends invites to the staff who can\'t sign in yet', async () => {
    api.get.mockResolvedValue(job('imported', { sign_in: { pending: 4, invited: 0, email_ready: true } }))
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    expect(find('import-sign-in-pending').text()).toContain('import-done-sign-in-pending')
    api.invite.mockResolvedValue({ sent: 4, failed: 0, sign_in: { pending: 4, invited: 4, email_ready: true } })
    await find('import-sign-in-send').trigger('click')
    await flushPromises()
    expect(api.invite).toHaveBeenCalledWith('wimp-1')
    expect(find('import-sign-in-sent').exists()).toBe(true)
  })

  it('says invites need mail, with the CLI fallback', async () => {
    api.get.mockResolvedValue(job('imported', { sign_in: { pending: 4, invited: 0, email_ready: false } }))
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    expect(find('import-sign-in-email-off').exists()).toBe(true)
    expect(find('workspace-signin-cli').exists()).toBe(true)
    expect(find('import-sign-in-send').exists()).toBe(false)
  })

  it('lists the next steps with a link each, and keeps the ticks', async () => {
    api.get.mockResolvedValue(job('imported'))
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    const items = wrapper!.findAll('[data-test="workspace-checklist"] li')
    expect(items.map((li) => li.attributes('data-item'))).toEqual(['smtp', 'channels', 'sso', 'webhooks', 'plugins', 'rules', 'tokens'])
    await items[0].find('input').setValue(true)
    expect(localStorage.getItem('nosdesk.workspace-import.wimp-1.done')).toBe('["smtp"]')
  })

  it('retries the files that didn\'t restore', async () => {
    vi.useFakeTimers()
    const partial = job('failed', {
      archive: { name: 'a.nosdesk', size: 10, sealed: false, stored: true },
      files_restored: 1612,
      error: { code: 'files_partial', message: '' },
      actions: { check: false, confirm: false, retry_files: true, reset: true, invite: true },
    })
    partial.report = { ...partial.report!, files: { restored: 1612, present: 0, skipped: [], failed: 'x' } }
    api.get.mockResolvedValueOnce(partial)
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    expect(find('workspace-done-partial').exists()).toBe(true)
    api.retryFiles.mockResolvedValue(job('importing', { stage: 'files' }))
    api.get.mockResolvedValue(job('imported'))
    await find('workspace-retry-files').trigger('click')
    await flushPromises()
    expect(api.retryFiles).toHaveBeenCalledWith('wimp-1', { password: undefined })
    await vi.advanceTimersByTimeAsync(2100)
    await flushPromises()
    expect(find('workspace-done-complete').exists()).toBe(true)
  })

  it('resets to untouched behind the typed name', async () => {
    api.get.mockResolvedValue(job('imported'))
    await mountAt('/admin/data-import/workspace?job=wimp-1')
    await find('workspace-reset-button').trigger('click')
    expect(find('workspace-reset-stakes').exists()).toBe(true)
    expect(find('confirm-ok').attributes('disabled')).toBeDefined()
    await find('workspace-reset-name').setValue('Acme Helpdesk')
    api.reset.mockResolvedValue(job('reset'))
    await find('confirm-ok').trigger('click')
    await flushPromises()
    expect(api.reset).toHaveBeenCalledWith('wimp-1')
    expect(find('workspace-done-reset').exists()).toBe(true)
    await find('workspace-import-again').trigger('click')
    await flushPromises()
    expect(find('workspace-archive').exists()).toBe(true)
    expect(router.currentRoute.value.query.job).toBeUndefined()
  })
})
