/**
 * Test doubles for the "Confirm it's you" flow: a BroadcastChannel that
 * delivers in-process, and a status builder.
 */
import type { StepUpStatus } from '@/services/stepUpService'

export class FakeBroadcastChannel {
  static instances: FakeBroadcastChannel[] = []
  onmessage: ((event: MessageEvent) => void) | null = null
  closed = false
  posted: unknown[] = []

  constructor(public name: string) {
    FakeBroadcastChannel.instances.push(this)
  }

  postMessage(data: unknown) {
    this.posted.push(data)
  }

  close() {
    this.closed = true
  }

  /** Deliver `data` to every open channel of this name, as another tab would. */
  static deliver(name: string, data: unknown) {
    for (const ch of FakeBroadcastChannel.instances) {
      if (ch.name === name && !ch.closed) ch.onmessage?.({ data } as MessageEvent)
    }
  }

  static reset() {
    FakeBroadcastChannel.instances = []
  }
}

export function stepUpStatus(overrides: Partial<StepUpStatus> = {}): StepUpStatus {
  return {
    confirmed_until: null,
    window_minutes: 10,
    methods: ['password'],
    provider: null,
    email: 'kyle@example.com',
    ...overrides,
  }
}

export function inMinutes(minutes: number): string {
  return new Date(Date.now() + minutes * 60_000).toISOString()
}
