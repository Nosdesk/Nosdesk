/**
 * Field auto-save with separate preview and commit channels.
 *
 * Decouples real-time mirroring from persistence: every `update()`
 * triggers a short-debounce *preview* (broadcast to other viewers,
 * no persistence), while *commit* (debounced longer, plus a hard
 * max-wait cap, plus `commitNow()` for blur) is the only path that
 * hits the backend's mutate-and-record endpoint.
 *
 * This is the pattern needed when you want a typed field to feel
 * live to other viewers but to leave a single entry in the activity
 * log per editing session, not one per debounced keystroke. The
 * Yjs-backed article body solves the same problem at a much higher
 * weight class; plain string fields don't need a CRDT, so we
 * coordinate two simple timers and a "is this a no-op" comparator.
 *
 * The composable is owner-agnostic: caller supplies `preview` and
 * `commit` callbacks, plus an optional equality check so consumers
 * can skip a commit when the value matches what they last
 * committed (and to skip a preview that would echo identical to
 * the last preview).
 *
 * Lifecycle: `dispose()` cancels pending timers without flushing
 * (for unmount paths where the caller already triggered
 * `commitNow()` via `beforeRouteLeave` or `onBeforeUnmount`).
 */
import { onBeforeUnmount } from 'vue'

export interface FieldAutoSaveOptions<T> {
  /**
   * Broadcast the in-flight value to other viewers. Should not
   * persist. Skipped automatically when the value is unchanged
   * since the previous preview.
   */
  preview: (value: T) => Promise<void> | void
  /**
   * Persist the final value and emit the durable activity event.
   * Skipped when the value equals the last successful commit.
   */
  commit: (value: T) => Promise<void> | void
  /**
   * Equality function for skipping no-op previews and commits.
   * Defaults to `Object.is`, which is the right call for primitive
   * fields (strings, numbers) and avoids surprising deep-compare
   * for objects.
   */
  isEqual?: (a: T, b: T) => boolean
  /**
   * Trailing-edge debounce for the preview channel. Tuned for
   * "feels live" without firing per-keystroke: short enough that a
   * typing pause flushes within a frame or two, long enough that a
   * burst of fast typing coalesces. Default 150ms.
   */
  previewMs?: number
  /**
   * Trailing-edge debounce for the commit channel. Each `update()`
   * resets this, so a continuous typing run does not commit. Pair
   * with `commitMaxMs` for the safety net. Default 3000ms.
   */
  commitIdleMs?: number
  /**
   * Hard cap on time since the first un-committed change. Forces a
   * commit even if the user keeps typing past `commitIdleMs`. This
   * is the durability guard: if the tab crashes or the network
   * drops, at most `commitMaxMs` of typing is unsaved. Default
   * 8000ms.
   */
  commitMaxMs?: number
  /**
   * Initial baseline for the "last committed" reference. Pass the
   * value the field was loaded with so the first commit is skipped
   * if the user opens the field, types, and reverts back to the
   * original.
   */
  initial?: T
}

export interface FieldAutoSave<T> {
  /** Call on every input change. */
  update(value: T): void
  /** Force commit now (e.g. on blur). Resolves when the commit
   *  finishes or is skipped as a no-op. */
  commitNow(): Promise<void>
  /**
   * Set the "last committed" reference without scheduling any
   * network work. Use this when the consumer learns the
   * authoritative value asynchronously (e.g. on focus, after the
   * ticket finishes loading) so a user who types and reverts back
   * to the original correctly skips a redundant commit.
   */
  seed(value: T): void
  /** Cancel pending timers without flushing. Called automatically
   *  on component unmount; expose for tests / explicit cleanup. */
  dispose(): void
}

export function useFieldAutoSave<T>(
  options: FieldAutoSaveOptions<T>,
): FieldAutoSave<T> {
  const previewMs = options.previewMs ?? 150
  const commitIdleMs = options.commitIdleMs ?? 3000
  const commitMaxMs = options.commitMaxMs ?? 8000
  const isEqual = options.isEqual ?? Object.is

  // Latest value handed to `update()`. We always work off this
  // ref rather than closing over the argument so timer callbacks
  // pick up the most recent draft.
  let latest: { value: T } | null =
    options.initial !== undefined ? { value: options.initial } : null
  let lastPreviewed: { value: T } | null = null
  let lastCommitted: { value: T } | null =
    options.initial !== undefined ? { value: options.initial } : null

  let previewTimer: ReturnType<typeof setTimeout> | null = null
  let commitIdleTimer: ReturnType<typeof setTimeout> | null = null
  let commitMaxTimer: ReturnType<typeof setTimeout> | null = null

  function clearPreview() {
    if (previewTimer) {
      clearTimeout(previewTimer)
      previewTimer = null
    }
  }
  function clearCommit() {
    if (commitIdleTimer) {
      clearTimeout(commitIdleTimer)
      commitIdleTimer = null
    }
    if (commitMaxTimer) {
      clearTimeout(commitMaxTimer)
      commitMaxTimer = null
    }
  }

  async function flushPreview(): Promise<void> {
    if (!latest) return
    if (lastPreviewed && isEqual(lastPreviewed.value, latest.value)) return
    const value = latest.value
    try {
      await options.preview(value)
      lastPreviewed = { value }
    } catch (err) {
      // Preview is best-effort. A failed preview should not break
      // commit, retries, or the input. Log via console so the
      // failure is observable in dev tools without surfacing to
      // the user.
      console.warn('[useFieldAutoSave] preview failed', err)
    }
  }

  async function flushCommit(): Promise<void> {
    clearCommit()
    if (!latest) return
    if (lastCommitted && isEqual(lastCommitted.value, latest.value)) return
    const value = latest.value
    try {
      await options.commit(value)
      lastCommitted = { value }
      // A commit settles the preview baseline too: the next
      // preview is meaningful only if the value moves *past* the
      // committed state.
      lastPreviewed = { value }
    } catch (err) {
      // Commit failures bubble up to the caller via its own
      // `commit` callback's error handling. Re-throw so callers
      // awaiting `commitNow()` can react.
      throw err
    }
  }

  function update(value: T): void {
    latest = { value }

    clearPreview()
    previewTimer = setTimeout(() => {
      previewTimer = null
      void flushPreview()
    }, previewMs)

    if (commitIdleTimer) clearTimeout(commitIdleTimer)
    commitIdleTimer = setTimeout(() => {
      commitIdleTimer = null
      void flushCommit()
    }, commitIdleMs)

    // The max timer is set on the *first* change since the last
    // commit and not reset on subsequent keystrokes, so it caps
    // total time-since-first-change rather than time-since-last
    // keystroke. That's the durability guarantee.
    if (!commitMaxTimer) {
      commitMaxTimer = setTimeout(() => {
        commitMaxTimer = null
        void flushCommit()
      }, commitMaxMs)
    }
  }

  async function commitNow(): Promise<void> {
    clearPreview()
    await flushCommit()
  }

  function seed(value: T): void {
    lastCommitted = { value }
    lastPreviewed = { value }
    // Don't touch `latest`: a baseline shouldn't pretend the user
    // typed. If timers are already scheduled (from a prior
    // `update`), leave them alone — they'll evaluate against the
    // new baseline when they fire.
  }

  function dispose(): void {
    clearPreview()
    clearCommit()
  }

  // Best-effort cleanup. Callers that need a final save on unmount
  // should await `commitNow()` from a navigation guard before
  // unmount fires; this just stops timers from leaking.
  onBeforeUnmount(dispose)

  return { update, commitNow, seed, dispose }
}
