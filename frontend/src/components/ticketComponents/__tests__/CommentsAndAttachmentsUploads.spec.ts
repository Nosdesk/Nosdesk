import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'
import { defineComponent, h, nextTick } from 'vue'
import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { FluentBundle } from '@fluent/bundle'
import { createFluentVue } from 'fluent-vue'
import { createPinia } from 'pinia'
import { ConfigProvider, TooltipProvider } from 'reka-ui'
import type { AxiosRequestConfig } from 'axios'
// Its browser shims (localStorage, matchMedia) for the stores.
import '@/test/mountWithProviders'

interface Call {
  form: FormData
  config: AxiosRequestConfig
  resolve: (v: unknown) => void
  reject: (e: unknown) => void
}
const calls: Call[] = []
vi.mock('@nosdesk/core/apiClient', () => ({
  default: {
    interceptors: { request: { use: () => 0 }, response: { use: () => 0 } },
    defaults: { headers: { common: {} } },
    get: () => new Promise(() => {}),
    post: (_url: string, form: FormData, config: AxiosRequestConfig) =>
      new Promise((resolve, reject) => {
        calls.push({ form, config, resolve, reject })
        config.signal?.addEventListener('abort', () =>
          reject(Object.assign(new Error('canceled'), { code: 'ERR_CANCELED' })),
        )
      }),
  },
}))

// The real recorder loads a media encoder that needs a Worker.
vi.mock('@/components/ticketComponents/VoiceRecorder.vue', async () => {
  const { defineComponent, h } = await import('vue')
  return {
    default: defineComponent({
      name: 'VoiceRecorder',
      emits: ['recording-complete', 'cancel'],
      setup: () => () => h('div', { class: 'recorder-stub' }),
    }),
  }
})

import CommentsAndAttachments from '../CommentsAndAttachments.vue'
import type { StagedAttachment } from '@nosdesk/core/stores/ticketUi'

beforeAll(() => {
  let n = 0
  URL.createObjectURL = vi.fn(() => `blob:preview-${++n}`)
  URL.revokeObjectURL = vi.fn()
})
afterEach(() => {
  calls.length = 0
  document.body.innerHTML = ''
})

// The editor and the tile are heavy; these stand in with the same events and
// the tile's status slot, which is what the composer fills.
const SimpleEditorStub = defineComponent({
  name: 'SimpleEditor',
  props: { modelValue: { type: String, default: '' } },
  emits: ['update:modelValue', 'submit', 'paste-files'],
  setup: () => () => h('div', { class: 'editor-stub' }),
})
const AttachmentPreviewStub = defineComponent({
  name: 'AttachmentPreview',
  props: { attachment: { type: Object, required: true } },
  emits: ['delete', 'submit'],
  setup: (props, { slots, emit }) => () =>
    h('div', { class: 'tile', 'data-name': (props.attachment as { name: string }).name }, [
      h('button', { class: 'tile-delete', type: 'button', onClick: () => emit('delete') }),
      slots.status?.(),
    ]),
})

function mountComposer(): VueWrapper {
  const Host = defineComponent({
    setup: () => () =>
      h(ConfigProvider, {}, () =>
        h(TooltipProvider, {}, () =>
          h(CommentsAndAttachments, { comments: [], currentUser: 'agent-uuid', ticketId: 101 }),
        ),
      ),
  })
  return mount(Host, {
    attachTo: document.body,
    global: {
      plugins: [createFluentVue({ bundles: [new FluentBundle('en-US')] }), createPinia()],
      stubs: {
        SimpleEditor: SimpleEditorStub,
        AttachmentPreview: AttachmentPreviewStub,
        CannedResponsePicker: true,
      },
    },
  })
}

async function pick(wrapper: VueWrapper, files: File[]) {
  const input = wrapper.find('input[type="file"]')
  Object.defineProperty(input.element, 'files', { value: files, configurable: true })
  await input.trigger('change')
  await flushPromises()
}

const send = (w: VueWrapper) => w.find('button[type="submit"]')
const composer = (w: VueWrapper) => w.findComponent(CommentsAndAttachments)
const stored = (id: number, name: string) => ({ data: [{ id, url: `/uploads/temp/${name}`, name }] })
const file = (name: string, type = 'text/plain') => new File(['hello'], name, { type })

describe('the reply composer', () => {
  it('uploads each picked file at once, with its progress', async () => {
    const w = mountComposer()
    await pick(w, [file('log.txt'), file('trace.txt')])

    expect(calls).toHaveLength(2)
    expect(w.findAll('[data-testid="upload-progress"]')).toHaveLength(2)

    calls[0].config.onUploadProgress?.({ loaded: 40, total: 100 } as never)
    await nextTick()
    const bar = w.find('.tile[data-name="log.txt"] [role="progressbar"]')
    expect(bar.attributes('aria-valuenow')).toBe('40')
    expect(bar.find('span').attributes('style')).toContain('width: 40%')
  })

  it('holds Send until every file has uploaded, then posts their ids', async () => {
    const w = mountComposer()
    await pick(w, [file('log.txt')])

    expect(send(w).attributes('disabled')).toBeDefined()
    expect(send(w).attributes('title')).toBe('ticket-comments-submit-waiting-uploads')
    // Enter in the editor doesn't get past it either.
    w.findComponent(SimpleEditorStub).vm.$emit('submit')
    expect(composer(w).emitted('addComment')).toBeUndefined()

    calls[0].resolve(stored(7, 'log.txt'))
    await flushPromises()
    expect(send(w).attributes('disabled')).toBeUndefined()
    expect(w.find('[data-testid="upload-progress"]').exists()).toBe(false)

    await send(w).trigger('submit')
    const [[payload]] = composer(w).emitted('addComment') as [[{ attachments: StagedAttachment[]; is_internal: boolean }]]
    expect(payload.attachments.map((a) => a.uploaded?.id)).toEqual([7])
    expect(payload.is_internal).toBe(false)
    expect(w.findAll('.tile')).toHaveLength(0)
    // Sending posts the ids; it uploads nothing again.
    expect(calls).toHaveLength(1)
  })

  it('sends an internal note with its uploaded files', async () => {
    const w = mountComposer()
    await w.findAll('button').find((b) => b.text() === 'ticket-comments-internal-note')!.trigger('click')
    await pick(w, [file('notes.txt')])
    calls[0].resolve(stored(8, 'notes.txt'))
    await flushPromises()

    await send(w).trigger('submit')
    const [[payload]] = composer(w).emitted('addComment') as [[{ attachments: StagedAttachment[]; is_internal: boolean }]]
    expect(payload.is_internal).toBe(true)
    expect(payload.attachments.map((a) => a.uploaded?.id)).toEqual([8])
  })

  it('cancels an upload and drops the file', async () => {
    const w = mountComposer()
    await pick(w, [file('big.mov', 'video/quicktime')])

    await w.find('[data-testid="upload-progress"] button').trigger('click')
    await flushPromises()

    expect(calls[0].config.signal?.aborted).toBe(true)
    expect(w.findAll('.tile')).toHaveLength(0)
    expect(send(w).attributes('disabled')).toBeDefined()
  })

  it('drops an uploaded file without uploading anything', async () => {
    const w = mountComposer()
    await pick(w, [file('a.txt'), file('b.txt')])
    calls[0].resolve(stored(1, 'a.txt'))
    calls[1].resolve(stored(2, 'b.txt'))
    await flushPromises()

    await w.find('.tile[data-name="a.txt"] .tile-delete').trigger('click')
    await send(w).trigger('submit')

    const [[payload]] = composer(w).emitted('addComment') as [[{ attachments: StagedAttachment[] }]]
    expect(payload.attachments.map((a) => a.uploaded?.id)).toEqual([2])
    expect(calls).toHaveLength(2)
  })

  it('says why an upload failed and retries it', async () => {
    const w = mountComposer()
    await pick(w, [file('a.txt')])
    calls[0].reject({ response: { status: 503, data: { code: 'SERVICE_UNAVAILABLE' } } })
    await flushPromises()

    const failed = w.find('[data-testid="upload-failed"]')
    expect(failed.text()).toContain('ticket-comments-upload-busy')
    expect(send(w).attributes('disabled')).toBeDefined()
    expect(send(w).attributes('title')).toBe('ticket-comments-submit-failed-uploads')

    await failed.findAll('button').find((b) => b.text() === 'ticket-comments-upload-retry')!.trigger('click')
    expect(calls).toHaveLength(2)
    calls[1].resolve(stored(9, 'a.txt'))
    await flushPromises()

    expect(w.find('[data-testid="upload-failed"]').exists()).toBe(false)
    expect(send(w).attributes('disabled')).toBeUndefined()
  })

  it('names a file the server refused', async () => {
    const w = mountComposer()
    await pick(w, [file('setup.bin')])
    calls[0].reject({ response: { status: 400, data: { code: 'FILE_TYPE_NOT_ALLOWED', file: 'setup.bin' } } })
    await flushPromises()

    expect(w.find('[data-testid="upload-failed"]').text()).toContain(
      'ticket-comments-attachment-type-not-allowed',
    )
  })

  it('uploads pasted images and voice notes too', async () => {
    const w = mountComposer()
    w.findComponent(SimpleEditorStub).vm.$emit('paste-files', [file('image.png', 'image/png')])
    await flushPromises()
    expect(calls).toHaveLength(1)
    expect((calls[0].form.get('files') as File).name).toMatch(/^pasted-.*\.png$/)

    calls[0].resolve(stored(1, 'pasted.png'))
    await flushPromises()
    await w.find('button[aria-label="ticket-comments-record-voice"]').trigger('click')
    w.findComponent({ name: 'VoiceRecorder' }).vm.$emit('recording-complete', {
      blob: new Blob(['a'], { type: 'audio/webm' }),
      duration: 2,
      transcription: 'Printer is jammed',
    })
    await flushPromises()
    expect(calls).toHaveLength(2)
    expect(calls[1].form.get('transcription')).toBe('Printer is jammed')
  })
})
