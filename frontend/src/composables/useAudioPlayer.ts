/**
 * Simple global audio player manager using EventTarget.
 *
 * This is the simplest Vue 3 pattern for "only one audio plays at a time":
 * - Uses native EventTarget for pub/sub (no external dependencies)
 * - Each player listens for 'pause-others' events
 * - When a player starts, it dispatches an event with its ID
 * - Other players receive the event and pause themselves
 *
 * Alternative approaches considered:
 * - Pinia store: Overkill for this simple use case
 * - provide/inject: Requires app-level setup, less portable
 * - Custom event bus: This IS a custom event bus, just using native APIs
 */

// Singleton event target for audio coordination
const audioEventTarget = new EventTarget()

let instanceCounter = 0

export function useAudioPlayer() {
  const playerId = `audio-player-${++instanceCounter}`

  /**
   * Listen for pause events from other players.
   * @param onPause Callback to pause this player
   * @returns Cleanup function to remove listener
   */
  const onPauseOthers = (onPause: () => void): (() => void) => {
    const handler = (event: Event) => {
      const customEvent = event as CustomEvent<string>
      // Only pause if the event is from a different player
      if (customEvent.detail !== playerId) {
        onPause()
      }
    }

    audioEventTarget.addEventListener('pause-others', handler)

    // Return cleanup function
    return () => {
      audioEventTarget.removeEventListener('pause-others', handler)
    }
  }

  /**
   * Notify all other players to pause (this player is starting).
   */
  const notifyPlayStarted = () => {
    audioEventTarget.dispatchEvent(
      new CustomEvent('pause-others', { detail: playerId })
    )
  }

  return {
    playerId,
    onPauseOthers,
    notifyPlayStarted
  }
}
