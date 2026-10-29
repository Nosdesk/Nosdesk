import { afterEach, describe, expect, it } from 'vitest'
import { createMemoryHistory, createRouter } from 'vue-router'
import { usePdfOverlay } from '@/composables/usePdfOverlay'

const stored = '/uploads/tickets/7/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb_invoice.pdf'

async function ticketRouter(start = '/tickets/12?tab=files') {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/tickets/:number', component: { render: () => null } },
      { path: '/', component: { render: () => null } },
    ],
  })
  await router.push(start)
  await router.isReady()
  return router
}

// The composable reads the live route; give it the router's reactive one.
async function setup(start?: string) {
  const router = await ticketRouter(start)
  const route = router.currentRoute
  const overlay = usePdfOverlay(router, new Proxy({}, { get: (_, key) => (route.value as never)[key] }) as never)
  return { router, overlay }
}

afterEach(async () => {
  // Close any unsaved file left open in the shared state.
  const { overlay } = await setup('/')
  overlay.close()
})

describe('usePdfOverlay', () => {
  it('opens a saved file over the page, keeping the rest of the address', async () => {
    const { router, overlay } = await setup()
    overlay.open({ src: stored, name: 'invoice.pdf' })
    await router.isReady()
    await new Promise((r) => setTimeout(r))
    expect(router.currentRoute.value.query).toEqual({ tab: 'files', pdf: stored })
    expect(overlay.source.value).toMatchObject({ src: stored, name: 'invoice.pdf', page: null, local: false })
  })

  it('goes back on close when opening added the history entry', async () => {
    const { router, overlay } = await setup()
    overlay.open({ src: stored, name: 'invoice.pdf' })
    await new Promise((r) => setTimeout(r))
    overlay.close()
    await new Promise((r) => setTimeout(r, 10))
    expect(router.currentRoute.value.fullPath).toBe('/tickets/12?tab=files')
    expect(overlay.source.value).toBeNull()
  })

  it('drops the file from the address when the link was opened cold', async () => {
    const { router, overlay } = await setup(`/tickets/12?pdf=${encodeURIComponent(stored)}&page=2`)
    expect(overlay.source.value).toMatchObject({ src: stored, page: 2 })
    overlay.close()
    await new Promise((r) => setTimeout(r, 10))
    expect(router.currentRoute.value.fullPath).toBe('/tickets/12')
  })

  it("opens an unsaved file from memory, and won't open a file it may not", async () => {
    const { router, overlay } = await setup()
    overlay.open({ src: 'https://evil.example/x.pdf', name: 'x.pdf' })
    await new Promise((r) => setTimeout(r))
    expect(router.currentRoute.value.query.pdf).toBeUndefined()

    overlay.open({ src: 'blob:http://localhost/abc', name: 'draft.pdf' })
    expect(overlay.source.value).toMatchObject({ src: 'blob:http://localhost/abc', name: 'draft.pdf', local: true })
    expect(router.currentRoute.value.query.pdf).toBeUndefined()
    overlay.close()
    expect(overlay.source.value).toBeNull()
  })
})
