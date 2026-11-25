/**
 * The iPhone's Text Size setting, in the iOS app.
 *
 * WKWebView does not scale a page for the system text size (WebKit closed the
 * request as WONTFIX), but it does resolve the `-apple-system-body` font to the
 * current Dynamic Type body size: 17px at the default (Large), 19px at
 * xLarge, and up to 53px at the largest accessibility size. A hidden probe
 * reads that size, and the root font size follows it through
 * `--device-text-scale` (main.css), so rem type and spacing grow together, as
 * page zoom does.
 *
 * - Sizes below the default don't shrink the app.
 * - The scale stops at 1.35 so a phone layout holds: a 402px iPhone keeps
 *   about 298px of rem space.
 * - Android needs none of this: its WebView already applies the system font
 *   scale to page text.
 *
 * The probe has a real box, so a ResizeObserver sees a change made from
 * Control Center while the app stays open; a return from Settings measures
 * again too.
 */
import { isTauriRuntime } from './index'

/** The Dynamic Type body size at the default setting, in CSS px. */
export const DEFAULT_BODY_PX = 17
export const MAX_DEVICE_TEXT_SCALE = 1.35

const PROPERTY = '--device-text-scale'

/** The root scale for a measured body size: never below 1, never above the cap. */
export function scaleForBodySize(px: number | null): number {
  if (px == null || !Number.isFinite(px) || px <= 0) return 1
  const scale = Math.min(MAX_DEVICE_TEXT_SCALE, Math.max(1, px / DEFAULT_BODY_PX))
  return Math.round(scale * 1000) / 1000
}

function createProbe(): HTMLElement {
  const probe = document.createElement('span')
  probe.setAttribute('aria-hidden', 'true')
  probe.setAttribute('data-text-size-probe', '')
  probe.textContent = 'M'
  probe.style.cssText =
    'position:fixed;top:0;left:0;visibility:hidden;pointer-events:none;white-space:nowrap;font:-apple-system-body'
  document.documentElement.appendChild(probe)
  return probe
}

function readPx(el: HTMLElement): number | null {
  const px = parseFloat(getComputedStyle(el).fontSize)
  return Number.isFinite(px) ? px : null
}

export interface DeviceTextSizeOptions {
  /** Reads the body size in px; the probe by default. */
  measure?: () => number | null
}

/**
 * Set the root scale from the phone's text size now and whenever it changes.
 * A no-op (returning a no-op) anywhere but the iOS app. Returns a stop
 * function that removes the probe and listeners and the scale.
 */
export function followDeviceTextSize(
  os: string | null,
  options: DeviceTextSizeOptions = {},
): () => void {
  if (!isTauriRuntime() || os !== 'ios') return () => {}

  const root = document.documentElement
  const probe = options.measure ? null : createProbe()
  const measure = options.measure ?? (() => readPx(probe!))

  const apply = () => {
    root.style.setProperty(PROPERTY, String(scaleForBodySize(measure())))
  }
  const onVisibility = () => {
    if (document.visibilityState === 'visible') apply()
  }

  apply()
  document.addEventListener('visibilitychange', onVisibility)
  window.addEventListener('resize', apply)
  const observer =
    probe && typeof ResizeObserver !== 'undefined' ? new ResizeObserver(apply) : null
  if (observer && probe) observer.observe(probe)

  return () => {
    document.removeEventListener('visibilitychange', onVisibility)
    window.removeEventListener('resize', apply)
    observer?.disconnect()
    probe?.remove()
    root.style.removeProperty(PROPERTY)
  }
}
