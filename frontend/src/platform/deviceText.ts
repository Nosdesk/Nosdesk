/**
 * The phone's text size and Bold Text, in the native app.
 *
 * Neither app webview follows these settings properly by itself (WKWebView
 * ignores Dynamic Type; Android's WebView scales every font by itself, inside
 * boxes laid out for the default), so the native `device-text` plugin reports
 * them. While the app follows them (`FOLLOW_DEVICE_TEXT`), this module tells
 * the plugin so (Android then stops its own scaling) and puts them on the root
 * element for CSS:
 *
 * - `--device-text-scale` on `<html>`: body text relative to the platform
 *   default, unitless, to 4 decimals (iOS xLarge is 19/17, 1.1176). Values
 *   below 1 are passed through; the type tokens decide whether to floor them.
 * - `data-bold-text="true"` on `<html>` while Bold Text is on, absent otherwise.
 *
 * A browser gets neither: browser zoom and font settings already reach rem.
 *
 * `deviceText` holds the current values for the Text size settings page,
 * whether or not the app follows them.
 */
import { readonly, shallowRef } from 'vue'
import { isTauriRuntime } from './runtime'

/**
 * Whether the app applies the device text settings. Off until the layouts are
 * ready for larger text: the values are still read and exposed, nothing is
 * written to the page, and Android's WebView keeps scaling text by itself.
 */
export const FOLLOW_DEVICE_TEXT = false

export interface DeviceText {
  /** Body text size relative to the platform default, 0.5 to 2, to 4 decimals. */
  scale: number
  bold: boolean
}

export const DEVICE_TEXT_SCALE_PROPERTY = '--device-text-scale'
export const BOLD_TEXT_ATTRIBUTE = 'data-bold-text'

const MIN_SCALE = 0.5
const MAX_SCALE = 2

/**
 * How long launch waits for the first read. The launch splash covers the
 * wait, so the first screen draws at the right size; a slow or missing plugin
 * must not hold the app back longer than this.
 */
const FIRST_READ_TIMEOUT_MS = 500

const current = shallowRef<DeviceText | null>(null)

/** The device's text settings. `null` in a browser, or until the first read. */
export const deviceText = readonly(current)

/** A plugin payload as settings, or `null` when it is not one. */
export function parseDeviceText(raw: unknown): DeviceText | null {
  if (typeof raw !== 'object' || raw === null) return null
  const { scale, bold } = raw as { scale?: unknown; bold?: unknown }
  if (typeof scale !== 'number' || !Number.isFinite(scale) || typeof bold !== 'boolean') return null
  const bounded = Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale))
  return { scale: Math.round(bounded * 10000) / 10000, bold }
}

export function applyDeviceText(settings: DeviceText, root: HTMLElement = document.documentElement): void {
  root.style.setProperty(DEVICE_TEXT_SCALE_PROPERTY, String(settings.scale))
  if (settings.bold) root.setAttribute(BOLD_TEXT_ATTRIBUTE, 'true')
  else root.removeAttribute(BOLD_TEXT_ATTRIBUTE)
}

/** Whether the page applies the settings: on once the plugin has taken over. */
let applying = false

function receive(raw: unknown): void {
  const settings = parseDeviceText(raw)
  if (!settings) return
  const previous = current.value
  if (previous && previous.scale === settings.scale && previous.bold === settings.bold) return
  current.value = settings
  if (applying) applyDeviceText(settings)
}

function startApplying(): void {
  applying = true
  if (current.value) applyDeviceText(current.value)
}

function withTimeout(work: Promise<unknown>, ms: number): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms)
    const done = () => {
      clearTimeout(timer)
      resolve()
    }
    work.then(done, done)
  })
}

let stop: (() => void) | null = null

/**
 * Read the settings and follow changes. Call once at startup, before the app
 * mounts. Never throws, and waits at most `FIRST_READ_TIMEOUT_MS`.
 */
export async function initDeviceText({ follow = FOLLOW_DEVICE_TEXT }: { follow?: boolean } = {}): Promise<void> {
  if (stop || !isTauriRuntime()) return
  stop = () => {}
  try {
    const native = await import('@nosdesk/mobile/deviceText')
    // A read answered after a change event would put the older value back.
    let changes = 0
    const read = () => {
      const seen = changes
      return native.getDeviceText().then(
        (settings) => {
          if (seen === changes) receive(settings)
        },
        () => {},
      )
    }
    // Live changes (Control Center's Text Size, a split screen Settings app).
    const unlisten = native
      .onDeviceTextChange((settings) => {
        changes += 1
        receive(settings)
      })
      .catch(() => () => {})
    // A change made in the Settings app while this one was in the background.
    const onVisible = () => {
      if (document.visibilityState === 'visible') void read()
    }
    document.addEventListener('visibilitychange', onVisible)
    stop = () => {
      document.removeEventListener('visibilitychange', onVisible)
      void unlisten.then((off) => off())
    }
    // Android's WebView must stop scaling text before the page does it, or the
    // two multiply. A plugin that cannot take over leaves the page alone.
    const first = follow ? native.followDeviceText().then(startApplying, () => {}).then(read) : read()
    await withTimeout(first, FIRST_READ_TIMEOUT_MS)
  } catch {
    // Plugin absent (desktop dev shell): the app keeps its default size.
  }
}

/** Stop following changes. For tests; the app follows them for its lifetime. */
export function stopDeviceText(): void {
  stop?.()
  stop = null
  applying = false
}
