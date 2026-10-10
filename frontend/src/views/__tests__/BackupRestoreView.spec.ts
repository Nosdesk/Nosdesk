import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const manifest = {
  backup_format_version: 2,
  nosdesk_version: '1.1.0',
  schema_hash: 'abc',
  created_at: '2026-10-09T00:00:00Z',
  tables: { tickets: { count: 4 } },
  files: { total_count: 2, total_size_bytes: 91 },
}

const service = vi.hoisted(() => ({
  getJobs: vi.fn(),
  uploadRestore: vi.fn(),
  getRestorePreview: vi.fn(),
  unlockRestorePreview: vi.fn(),
  executeRestore: vi.fn(),
}))
vi.mock('@/services/backupService', () => ({ default: service, backupService: service }))

import BackupRestoreView from '@/views/BackupRestoreView.vue'

let wrapper: VueWrapper | null = null
beforeEach(() => {
  for (const fn of Object.values(service)) fn.mockReset()
  service.getJobs.mockResolvedValue([])
  service.uploadRestore.mockResolvedValue({ id: 'job-1' })
  service.executeRestore.mockResolvedValue({ success: true, files_restored: 2, message: '' })
})
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function upload() {
  wrapper = mountWithProviders(BackupRestoreView)
  await flushPromises()
  const input = wrapper.find('input[type="file"]')
  const file = new File(['zip'], 'backup.zip', { type: 'application/zip' })
  Object.defineProperty(input.element, 'files', { value: [file] })
  await input.trigger('change')
  await flushPromises()
  return wrapper
}

const restoreButton = (w: VueWrapper) => w.find('[data-test="restore-execute"]')

describe('BackupRestoreView restoring an encrypted backup', () => {
  it('asks for the password, then shows the backup and restores with it', async () => {
    service.getRestorePreview.mockResolvedValue({
      encrypted: true,
      password_required: true,
      manifest: null,
      warnings: [],
    })
    service.unlockRestorePreview.mockResolvedValue({
      encrypted: true,
      password_required: false,
      manifest,
      warnings: [],
    })
    const w = await upload()

    expect(w.find('[data-test="restore-unlock"]').exists()).toBe(true)
    expect(restoreButton(w).exists()).toBe(false)

    await w.find('[data-test="restore-password"] input').setValue('open-sesame')
    await w.find('[data-test="restore-unlock"]').trigger('click')
    await flushPromises()

    expect(service.unlockRestorePreview).toHaveBeenCalledWith('job-1', 'open-sesame')
    expect(w.text()).toContain('tickets: 4')
    expect(w.find('[data-test="restore-no-credentials"]').exists()).toBe(false)

    await restoreButton(w).trigger('click')
    await flushPromises()
    expect(service.executeRestore).toHaveBeenCalledWith('job-1', { password: 'open-sesame' })
  })
})

describe('BackupRestoreView restoring a backup without sensitive data', () => {
  it('names the sign-in loss and restores only once that is confirmed', async () => {
    service.getRestorePreview.mockResolvedValue({
      encrypted: false,
      password_required: false,
      manifest,
      warnings: [],
    })
    const w = await upload()

    expect(w.find('[data-test="restore-no-credentials"]').exists()).toBe(true)
    expect(restoreButton(w).attributes('disabled')).toBeDefined()

    await w.find('[data-test="restore-no-credentials"] input[type="checkbox"]').setValue(true)
    expect(restoreButton(w).attributes('disabled')).toBeUndefined()

    await restoreButton(w).trigger('click')
    await flushPromises()
    expect(service.executeRestore).toHaveBeenCalledWith('job-1', { password: undefined })
  })
})

describe('BackupRestoreView restoring a backup from an earlier version', () => {
  it('says the backup is upgraded as it restores, and which tables it replaces', async () => {
    service.getRestorePreview.mockResolvedValue({
      encrypted: true,
      password_required: false,
      manifest: { ...manifest, nosdesk_version: '1.0.12' },
      upgrade: {
        from_version: '1.0.12',
        to_version: '1.1.0',
        replaced_tables: ['workspace_widget_settings', 'ticket_ratings'],
      },
      warnings: [],
    })
    const w = await upload()

    const notice = w.find('[data-test="restore-upgrade"]')
    expect(notice.exists()).toBe(true)
    expect(notice.text()).toContain('admin-backup-restore-upgrade')
    const replaced = w.find('[data-test="restore-replaced-tables"]')
    expect(replaced.text()).toContain('workspace_widget_settings, ticket_ratings')
    expect(restoreButton(w).exists()).toBe(true)
  })

  it('refuses a backup this server cannot restore, at the preview', async () => {
    service.getRestorePreview.mockRejectedValue({
      response: { status: 400, data: { error: 'Can\'t restore', code: 'BACKUP_SCHEMA_UNKNOWN' } },
    })
    const w = await upload()

    const refusal = w.find('[data-test="restore-refused"]')
    expect(refusal.exists()).toBe(true)
    expect(refusal.text()).toContain('admin-backup-restore-schema-unknown')
    expect(restoreButton(w).exists()).toBe(false)
  })

  it.each([
    ['BACKUP_NEEDS_PRIVILEGES', 500, 'admin-backup-restore-needs-privileges'],
    ['BACKUP_RESTORE_IN_PROGRESS', 409, 'admin-backup-restore-in-progress'],
  ])('says why the restore was refused (%s)', async (code, status, key) => {
    service.getRestorePreview.mockResolvedValue({
      encrypted: true,
      password_required: false,
      manifest,
      upgrade: { from_version: '1.0.12', to_version: '1.1.0', replaced_tables: [] },
      warnings: [],
    })
    service.executeRestore.mockRejectedValue({ response: { status, data: { error: 'x', code } } })
    const w = await upload()
    expect(w.find('[data-test="restore-replaced-tables"]').exists()).toBe(false)
    await restoreButton(w).trigger('click')
    await flushPromises()

    expect(w.find('[data-test="restore-refused"]').text()).toContain(key)
  })
})
