/**
 * The guest-side gate on host messages.
 *
 * Lives here rather than in @nosdesk/plugin-sdk because the runtime package is
 * the one wired up with vitest in CI, and it already depends on the SDK. The
 * predicate is pure so it can be exercised without a DOM.
 */
import { describe, expect, it } from 'vitest'
import { isHostMessage } from '@nosdesk/plugin-sdk'

// Stand-ins for WindowProxy; the gate only ever compares identity.
const hostWindow = { id: 'host' }
const selfWindow = { id: 'self' }
const otherWindow = { id: 'other' }

const APP = 'https://app.example'

describe('isHostMessage', () => {
  it('accepts a message from the embedding frame before the handshake', () => {
    expect(
      isHostMessage({ source: hostWindow, origin: APP }, { hostWindow, selfWindow, hostOrigin: null }),
    ).toBe(true)
  })

  it('rejects a message from any other window', () => {
    expect(
      isHostMessage({ source: otherWindow, origin: APP }, { hostWindow, selfWindow, hostOrigin: null }),
    ).toBe(false)
  })

  // Not framed: window.parent === window. There is no host, so an opener that
  // popped the runtime open cannot drive the handshake.
  it('rejects everything when the runtime is not framed', () => {
    expect(
      isHostMessage(
        { source: selfWindow, origin: APP },
        { hostWindow: selfWindow, selfWindow, hostOrigin: null },
      ),
    ).toBe(false)
  })

  it('accepts later messages from the origin that completed the handshake', () => {
    expect(
      isHostMessage({ source: hostWindow, origin: APP }, { hostWindow, selfWindow, hostOrigin: APP }),
    ).toBe(true)
  })

  // The mid-session rewrite this gate exists to stop.
  it('rejects a later message from a different origin once pinned', () => {
    expect(
      isHostMessage(
        { source: hostWindow, origin: 'https://evil.example' },
        { hostWindow, selfWindow, hostOrigin: APP },
      ),
    ).toBe(false)
  })
})
