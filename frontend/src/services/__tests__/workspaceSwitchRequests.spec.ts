import { describe, expect, it } from 'vitest'
import { passRequestGates, refuseResponse, rememberHostHeaders } from '@nosdesk/core/transport'
import { beginWorkspaceSwitch, setActiveWorkspaceSlug } from '@/services/activeWorkspace'

// An in-app switch holds requests until the new workspace is set, and a
// response to a request sent under a previous workspace is refused.

const flush = () => new Promise((r) => setTimeout(r, 0))

describe('workspace switch and requests', () => {
  it('holds requests from the start of a switch until the new slug is set', async () => {
    setActiveWorkspaceSlug('mercury')
    beginWorkspaceSwitch()
    setActiveWorkspaceSlug(null) // the reset clears the old workspace
    let sent = false
    void passRequestGates().then(() => {
      sent = true
    })
    await flush()
    expect(sent).toBe(false)

    setActiveWorkspaceSlug('venus')
    await flush()
    expect(sent).toBe(true)
  })

  it('does not hold requests outside a switch (sign-in, logout)', async () => {
    setActiveWorkspaceSlug(null)
    let sent = false
    void passRequestGates().then(() => {
      sent = true
    })
    await flush()
    expect(sent).toBe(true)
  })

  it('refuses a response for another workspace, never one sent without a workspace', () => {
    setActiveWorkspaceSlug('mercury')
    const oldRequest = {}
    rememberHostHeaders(oldRequest, { 'X-Nosdesk-Workspace': 'mercury' })
    const signIn = {}
    rememberHostHeaders(signIn, {})

    expect(refuseResponse(oldRequest)).toBeNull()
    setActiveWorkspaceSlug('venus')
    expect(refuseResponse(oldRequest)).toBe('workspace changed')
    expect(refuseResponse(signIn)).toBeNull()
    expect(refuseResponse(undefined)).toBeNull()
  })
})
