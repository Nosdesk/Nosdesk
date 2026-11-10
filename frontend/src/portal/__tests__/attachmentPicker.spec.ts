import { afterEach, describe, expect, it, vi } from 'vitest'
import { flushPromises, type VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

const service = vi.hoisted(() => ({ uploadFile: vi.fn() }))
vi.mock('@/portal/service', () => service)

import AttachmentPicker from '@/portal/components/AttachmentPicker.vue'

let wrapper: VueWrapper | null = null

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.clearAllMocks()
})

function file(name: string, bytes: number): File {
  const f = new File(['x'], name, { type: 'text/plain' })
  Object.defineProperty(f, 'size', { value: bytes })
  return f
}

async function pick(...files: File[]) {
  wrapper = mountWithProviders(AttachmentPicker, { modelValue: [] })
  const input = wrapper.find('input[type="file"]')
  Object.defineProperty(input.element, 'files', { value: files, configurable: true })
  await input.trigger('change')
  await flushPromises()
  return wrapper.findAll('[data-test="portal-attach-failed"]').map((p) => p.text())
}

describe('AttachmentPicker', () => {
  it('says a file is over the limit without sending it', async () => {
    const lines = await pick(file('big.txt', 12 * 1024 * 1024))
    expect(service.uploadFile).not.toHaveBeenCalled()
    expect(lines).toEqual(['portal-attach-too-large'])
  })

  it('says a file is over the limit when the server refuses its size', async () => {
    service.uploadFile.mockRejectedValue({
      response: { status: 413, data: { error: 'File exceeds 10MB limit' } },
    })
    const lines = await pick(file('notes.txt', 1024))
    expect(service.uploadFile).toHaveBeenCalled()
    expect(lines).toEqual(['portal-attach-too-large'])
  })

  it('keeps the general line for any other failure', async () => {
    service.uploadFile.mockRejectedValue({ response: { status: 500, data: {} } })
    const lines = await pick(file('notes.txt', 1024))
    expect(lines).toEqual(['portal-attach-failed'])
  })

  it('attaches a file under the limit', async () => {
    service.uploadFile.mockResolvedValue({ id: 7, name: 'notes.txt', file_size: 1024, mime_type: 'text/plain' })
    const lines = await pick(file('notes.txt', 1024))
    expect(lines).toEqual([])
    expect(wrapper!.findComponent(AttachmentPicker).emitted('update:modelValue')?.at(-1)).toEqual([
      [{ id: 7, name: 'notes.txt', file_size: 1024, mime_type: 'text/plain' }],
    ])
  })
})
