/**
 * Settings > Appearance > Text size, kept on this device.
 *
 * In the apps the size follows the device by default ("Use device text
 * size"): the native shell writes `--device-text-scale` (`deviceText.ts`) and
 * nothing here is set. Choosing a size of one's own writes
 * `--user-text-scale` on <html>, which the type tokens prefer (main.css), and
 * turning the switch back on removes it. Bold Text always follows the device;
 * it is not a setting here.
 *
 * A browser has no device size to follow (its own text setting already
 * reaches rem), so there the setting is the person's size alone, and 100%
 * writes nothing.
 *
 * Per device, in localStorage: the right size depends on the screen, and a
 * phone already carries its own setting from device to device.
 */
import { computed, reactive, readonly } from 'vue'
import { TEXT_SIZE_STOPS, composeScale, nearestStop } from '@/utils/textScale'
import { FOLLOW_DEVICE_TEXT, deviceText } from './deviceText'
import { isTauriRuntime } from './runtime'

export const USER_TEXT_SCALE_PROPERTY = '--user-text-scale'
const STORAGE_KEY = 'textSize'

export type TextSizePlatform = 'ios' | 'android' | 'web'

interface TextSizeState {
  /** The device sets the size (the apps only). */
  follow: boolean
  /** The person's own size, in percent, one of `TEXT_SIZE_STOPS`. */
  percent: number
}

const state = reactive<TextSizeState>({ follow: true, percent: 100 })

export const textSize = readonly(state)

/** How the setting names the device. */
export function textSizePlatform(): TextSizePlatform {
  if (!isTauriRuntime()) return 'web'
  return /android/i.test(navigator.userAgent) ? 'android' : 'ios'
}

/** Whether the device's size can be followed: in the apps, once the app
 *  applies the device text settings. */
export function canFollowDevice(): boolean {
  return FOLLOW_DEVICE_TEXT && textSizePlatform() !== 'web'
}

/** Whether the device sets the size: where it can, while switched on. */
export const followsDevice = computed(() => state.follow && canFollowDevice())

/** The size in force, in percent, as the type tokens apply it. */
export const textSizePercent = computed(() =>
  Math.round(composeScale(followsDevice.value, deviceText.value?.scale ?? 1, state.percent) * 100),
)

function isStop(value: unknown): value is number {
  return typeof value === 'number' && (TEXT_SIZE_STOPS as readonly number[]).includes(value)
}

function load(): void {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY)
    if (!raw) return
    const saved = JSON.parse(raw) as Partial<TextSizeState>
    if (typeof saved.follow === 'boolean') state.follow = saved.follow
    if (isStop(saved.percent)) state.percent = saved.percent
  } catch {
    // Storage blocked or a bad value: the designed size, following the device.
  }
}

function save(): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify({ follow: state.follow, percent: state.percent }))
  } catch {
    // Private browsing or storage turned off: the size holds for this visit.
  }
}

export function applyTextSize(root: HTMLElement = document.documentElement): void {
  const own = !followsDevice.value && !(!canFollowDevice() && state.percent === 100)
  if (own) root.style.setProperty(USER_TEXT_SCALE_PROPERTY, String(state.percent / 100))
  else root.style.removeProperty(USER_TEXT_SCALE_PROPERTY)
}

/** Read the saved size and apply it. Call once at startup, before mount. */
export function initTextSize(): void {
  load()
  applyTextSize()
}

/** The person chose a size: it becomes their own, and the device stops
 *  setting it. */
export function chooseTextSize(percent: number): void {
  if (!isStop(percent)) return
  state.percent = percent
  state.follow = false
  save()
  applyTextSize()
}

/** Turning following on returns to the device's size; turning it off keeps
 *  the size in force as the person's own. */
export function setFollowDeviceText(follow: boolean): void {
  if (!follow) state.percent = TEXT_SIZE_STOPS[nearestStop(textSizePercent.value)]
  state.follow = follow
  save()
  applyTextSize()
}
