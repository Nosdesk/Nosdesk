import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'

// A download link is a plain link on the web, where the browser saves the
// file. In the app, where the webview ignores `download`, a tap saves through
// the platform instead, shows it is busy, and reports a failure.
const files = vi.hoisted(() => ({ saveFile: vi.fn(async () => ({ completed: true })) }))
vi.mock('@/platform/files', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/platform/files')>()),
  saveFile: files.saveFile,
}))
const toast = vi.hoisted(() => ({ error: vi.fn() }))
vi.mock('@nosdesk/core/stores/toast', () => ({ useToastStore: () => toast }))

const DownloadLink = (await import('@/components/common/DownloadLink.vue')).default

const inApp = (on: boolean) => {
  ;(globalThis as { isTauri?: boolean }).isTauri = on
}

const mountLink = (props: Record<string, unknown>) => {
  setActivePinia(createPinia())
  return mount(DownloadLink, {
    props,
    attrs: { class: 'p-1.5', 'aria-label': 'Download a.wav' },
    slots: { default: '<span class="icon">icon</span>' },
    // No messages: a message reads as its key.
    global: { plugins: [createFluentVue({ bundles: [new FluentBundle('en-US')] })] },
  })
}

afterEach(() => {
  inApp(false)
  files.saveFile.mockReset()
  files.saveFile.mockResolvedValue({ completed: true })
  toast.error.mockClear()
})

describe('DownloadLink', () => {
  it('is a plain download link on the web, left to the browser', async () => {
    const link = mountLink({ href: '/uploads/tickets/1/a.wav', filename: 'a.wav' })
    const a = link.find('a')
    expect(a.attributes()).toMatchObject({
      href: '/api/files/tickets/1/a.wav',
      download: 'a.wav',
      class: 'p-1.5',
      'aria-label': 'Download a.wav',
    })
    expect(a.attributes('target')).toBeUndefined()
    const event = new MouseEvent('click', { cancelable: true })
    a.element.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(false)
    expect(files.saveFile).not.toHaveBeenCalled()
  })

  it('leaves the name to the server without one', () => {
    expect(mountLink({ href: '/api/admin/backup/download/7' }).find('a').attributes('download')).toBe('')
  })

  it('saves through the platform in the app, busy until done', async () => {
    inApp(true)
    let finish!: () => void
    files.saveFile.mockImplementationOnce(
      () => new Promise((resolve) => (finish = () => resolve({ completed: true }))),
    )
    const link = mountLink({ href: '/api/files/tickets/1/a.wav', filename: 'a.wav' })
    const event = new MouseEvent('click', { cancelable: true })
    link.find('a').element.dispatchEvent(event)
    await flushPromises()

    expect(event.defaultPrevented).toBe(true)
    expect(files.saveFile).toHaveBeenCalledWith('/api/files/tickets/1/a.wav', 'a.wav')
    expect(link.find('a').attributes('aria-busy')).toBe('true')
    expect(link.find('.icon').exists()).toBe(false)

    // A second tap while it runs starts nothing.
    await link.find('a').trigger('click')
    expect(files.saveFile).toHaveBeenCalledTimes(1)

    finish()
    await flushPromises()
    expect(link.find('a').attributes('aria-busy')).toBeUndefined()
    expect(link.find('.icon').exists()).toBe(true)
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('reports a failure, and not a dismissed sheet', async () => {
    inApp(true)
    files.saveFile.mockResolvedValueOnce({ completed: false })
    const link = mountLink({ href: '/api/files/tickets/1/a.wav' })
    await link.find('a').trigger('click')
    await flushPromises()
    expect(toast.error).not.toHaveBeenCalled()

    vi.spyOn(console, 'error').mockImplementation(() => {})
    files.saveFile.mockRejectedValueOnce(new Error('the server answered 403'))
    await link.find('a').trigger('click')
    await flushPromises()
    expect(toast.error).toHaveBeenCalledWith('common-file-save-failed')
  })
})
