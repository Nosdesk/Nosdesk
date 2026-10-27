import { afterEach, describe, expect, it, vi } from 'vitest'
import type { VueWrapper } from '@vue/test-utils'
import { mountWithProviders } from '@/test/mountWithProviders'

// The modal says who reads a report. On Cloud that is the Nosdesk team, and
// account or billing questions are pointed at support instead. Self-hosted, the
// report stays on the server for its administrators.

const { hosted } = await vi.hoisted(async () => {
  const { ref } = await import('vue')
  return { hosted: ref(true) }
})
vi.mock('@nosdesk/core/services/instanceConfig', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  isHostedDeploymentRef: hosted,
}))

import BugReportModal from '@/components/BugReportModal.vue'

const settle = () => new Promise((r) => setTimeout(r, 20))

let wrapper: VueWrapper | null = null
afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  document.body.innerHTML = ''
})

async function open(isHosted: boolean): Promise<string> {
  hosted.value = isHosted
  wrapper = mountWithProviders(BugReportModal, { isOpen: true })
  await settle()
  return document.body.textContent ?? ''
}

describe('the bug report modal', () => {
  it('on Cloud, says the report goes to the Nosdesk team and offers support', async () => {
    const text = await open(true)
    expect(text).toContain('bug-report-modal-attachments-hint-hosted')
    expect(text).toContain('bug-report-modal-support-hint')
    expect(document.querySelector('a[href="mailto:support@nosdesk.com"]')).toBeTruthy()
    expect(text).not.toContain('bug-report-modal-attachments-hint-self-hosted')
  })

  it('self-hosted, says the report stays on this server', async () => {
    const text = await open(false)
    expect(text).toContain('bug-report-modal-attachments-hint-self-hosted')
    expect(text).not.toContain('bug-report-modal-support-hint')
    expect(document.querySelector('a[href^="mailto:"]')).toBeNull()
  })
})
