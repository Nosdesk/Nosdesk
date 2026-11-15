import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { effectScope, nextTick, ref, type EffectScope } from 'vue'

const reduced = ref(false)
vi.mock('@/composables/useReducedMotion', () => ({ useReducedMotion: () => reduced }))

import {
  DISMISS_FRACTION,
  FLING_VELOCITY,
  SHEET_DISMISSING_ATTR,
  SLOP,
  VIDEO_CONTROLS_STRIP,
  backdropOpacity,
  classifyMove,
  dragOffset,
  dragOriginOf,
  glideDuration,
  isIgnoredTarget,
  isSheetPresentation,
  scrolledBetween,
  shouldDismiss,
  useSheetDismissGesture,
  velocityOf,
} from '@/composables/useSheetDismissGesture'

// jsdom lays nothing out; the panel reports the height a phone sheet has.
const PANEL_HEIGHT = 600

let desktop = false
let coarse = true

function mockMatchMedia() {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes('min-width') ? desktop : query.includes('pointer') ? coarse : false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    onchange: null,
    dispatchEvent: () => false,
  }))
}

describe('sheet dismiss decisions', () => {
  it('waits out the slop, then claims a mostly vertical move', () => {
    expect(classifyMove(3, 6, 'body')).toBe('pending')
    expect(classifyMove(0, SLOP, 'body')).toBe('drag')
    expect(classifyMove(4, 30, 'handle')).toBe('drag')
  })

  it('hands horizontal moves back to the platform from either origin', () => {
    expect(classifyMove(30, 10, 'body')).toBe('abort')
    expect(classifyMove(-30, 10, 'handle')).toBe('abort')
    expect(classifyMove(12, 12, 'handle')).toBe('abort')
  })

  it('treats an upward move from the body as a scroll but lets the handle stretch', () => {
    expect(classifyMove(0, -20, 'body')).toBe('abort')
    expect(classifyMove(0, -20, 'handle')).toBe('drag')
  })

  it('starts the panel from rest once the slop is spent and follows 1:1 downward', () => {
    expect(dragOffset(SLOP)).toBe(0)
    expect(dragOffset(SLOP + 1)).toBe(1)
    expect(dragOffset(210)).toBe(200)
  })

  it('rubber-bands upward toward a short stretch', () => {
    expect(dragOffset(-SLOP)).toBe(0)
    const a = dragOffset(-30)
    const b = dragOffset(-200)
    const c = dragOffset(-2000)
    expect(a).toBeLessThan(0)
    expect(b).toBeLessThan(a)
    expect(c).toBeLessThan(b)
    expect(c).toBeGreaterThan(-41)
  })

  it('dismisses past a quarter of the height on a slow drag', () => {
    expect(shouldDismiss(PANEL_HEIGHT * DISMISS_FRACTION - 1, PANEL_HEIGHT, 0)).toBe(false)
    expect(shouldDismiss(PANEL_HEIGHT * DISMISS_FRACTION, PANEL_HEIGHT, 0)).toBe(true)
  })

  it('dismisses on a downward fling at any distance and never on a flick back up', () => {
    expect(shouldDismiss(20, PANEL_HEIGHT, FLING_VELOCITY)).toBe(true)
    expect(shouldDismiss(20, PANEL_HEIGHT, FLING_VELOCITY - 0.01)).toBe(false)
    expect(shouldDismiss(PANEL_HEIGHT * 0.8, PANEL_HEIGHT, -FLING_VELOCITY)).toBe(false)
  })

  it('dims the backdrop in step with the drag', () => {
    expect(backdropOpacity(0, PANEL_HEIGHT)).toBe(1)
    expect(backdropOpacity(-20, PANEL_HEIGHT)).toBe(1)
    expect(backdropOpacity(PANEL_HEIGHT / 2, PANEL_HEIGHT)).toBeCloseTo(0.5)
    expect(backdropOpacity(PANEL_HEIGHT * 2, PANEL_HEIGHT)).toBe(0)
  })

  it('measures velocity over the recent window only', () => {
    expect(velocityOf([])).toBe(0)
    expect(velocityOf([{ t: 0, y: 0 }])).toBe(0)
    expect(velocityOf([{ t: 0, y: 0 }, { t: 50, y: 40 }])).toBeCloseTo(0.8)
    expect(velocityOf([{ t: 100, y: 10 }, { t: 100, y: 50 }])).toBe(0)
  })

  it('derives the glide from release speed, within bounds', () => {
    expect(glideDuration(400, 0)).toBe(240)
    expect(glideDuration(400, 10)).toBe(150)
    expect(glideDuration(400, 0.5)).toBe(300)
    expect(glideDuration(100, 0.5)).toBe(200)
  })

  it('finds a scrolled list between the finger and the panel', () => {
    const panel = document.createElement('div')
    const list = document.createElement('div')
    const row = document.createElement('div')
    panel.appendChild(list)
    list.appendChild(row)
    expect(scrolledBetween(row, panel)).toBe(false)
    Object.defineProperty(list, 'scrollTop', { value: 40, configurable: true })
    expect(scrolledBetween(row, panel)).toBe(true)
    // iOS reports a negative scrollTop mid-bounce: still at the top.
    Object.defineProperty(list, 'scrollTop', { value: -8, configurable: true })
    expect(scrolledBetween(row, panel)).toBe(false)
  })

  it('tells the handle from the body', () => {
    const panel = document.createElement('div')
    panel.innerHTML = '<header data-sheet-handle><h2>Title</h2></header><div class="body"><p>Text</p></div>'
    expect(dragOriginOf(panel.querySelector('h2'), panel)).toBe('handle')
    expect(dragOriginOf(panel.querySelector('p'), panel)).toBe('body')
    expect(dragOriginOf(null, panel)).toBe('body')
  })

  it('leaves fields, sliders, audio controls and opted-out elements alone', () => {
    const panel = document.createElement('div')
    panel.innerHTML = [
      '<input id="text">',
      '<textarea id="area"></textarea>',
      '<div id="rich" contenteditable="true"></div>',
      '<div id="locked" contenteditable="false"></div>',
      '<div id="slider" role="slider"></div>',
      '<audio id="audio" controls></audio>',
      '<video id="bare"></video>',
      '<div data-sheet-no-drag><span id="seek"></span></div>',
      '<button id="btn"></button>',
    ].join('')
    const q = (id: string) => panel.querySelector(`#${id}`)
    expect(isIgnoredTarget(q('text'), panel, 0)).toBe(true)
    expect(isIgnoredTarget(q('area'), panel, 0)).toBe(true)
    expect(isIgnoredTarget(q('rich'), panel, 0)).toBe(true)
    expect(isIgnoredTarget(q('locked'), panel, 0)).toBe(false)
    expect(isIgnoredTarget(q('slider'), panel, 0)).toBe(true)
    expect(isIgnoredTarget(q('audio'), panel, 0)).toBe(true)
    // A bare picture (custom controls) is a surface; a vertical drag over it dismisses.
    expect(isIgnoredTarget(q('bare'), panel, 0)).toBe(false)
    expect(isIgnoredTarget(q('seek'), panel, 0)).toBe(true)
    expect(isIgnoredTarget(q('btn'), panel, 0)).toBe(false)
  })

  it('gives the bottom strip of a video with native controls to the platform', () => {
    const panel = document.createElement('div')
    panel.innerHTML = '<video controls></video>'
    const video = panel.querySelector('video')!
    video.getBoundingClientRect = () => ({ top: 100, bottom: 400 }) as DOMRect
    expect(isIgnoredTarget(video, panel, 200)).toBe(false)
    expect(isIgnoredTarget(video, panel, 400 - VIDEO_CONTROLS_STRIP)).toBe(false)
    expect(isIgnoredTarget(video, panel, 400 - VIDEO_CONTROLS_STRIP + 1)).toBe(true)
    expect(isIgnoredTarget(video, panel, 399)).toBe(true)
  })

  it('is the sheet presentation only below the breakpoint with a coarse pointer', () => {
    mockMatchMedia()
    desktop = false
    coarse = true
    expect(isSheetPresentation()).toBe(true)
    coarse = false
    expect(isSheetPresentation()).toBe(false)
    coarse = true
    desktop = true
    expect(isSheetPresentation()).toBe(false)
  })
})

describe('useSheetDismissGesture on a sheet', () => {
  let scope: EffectScope | null = null
  let panel: HTMLElement
  let backdrop: HTMLElement
  let header: HTMLElement
  let body: HTMLElement
  let list: HTMLElement
  let dismissed: number
  let now: number
  let panelRef: ReturnType<typeof ref<HTMLElement | null>>

  beforeEach(async () => {
    desktop = false
    coarse = true
    reduced.value = false
    now = 1000
    dismissed = 0
    mockMatchMedia()
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    vi.spyOn(performance, 'now').mockImplementation(() => now)

    backdrop = document.createElement('div')
    panel = document.createElement('div')
    panel.innerHTML =
      '<header data-sheet-handle><h2>Title</h2></header>' +
      '<div class="body"><div class="list"><p>Row</p><input id="field"></div></div>'
    header = panel.querySelector('h2')!
    body = panel.querySelector('p')!
    list = panel.querySelector('.list')!
    panel.getBoundingClientRect = () =>
      ({ height: PANEL_HEIGHT, width: 390, top: 0, left: 0, bottom: PANEL_HEIGHT, right: 390, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
    document.body.append(backdrop, panel)

    panelRef = ref<HTMLElement | null>(panel)
    scope = effectScope()
    scope.run(() => {
      useSheetDismissGesture({
        panel: panelRef,
        backdrop,
        onDismiss: () => {
          dismissed++
        },
      })
    })
    await nextTick()
  })

  afterEach(() => {
    scope?.stop()
    scope = null
    vi.useRealTimers()
    vi.restoreAllMocks()
    document.body.innerHTML = ''
  })

  function pointer(type: string, target: Element, x: number, y: number) {
    target.dispatchEvent(
      new PointerEvent(type, {
        pointerId: 1,
        pointerType: 'mouse',
        isPrimary: true,
        button: 0,
        clientX: x,
        clientY: y,
        bubbles: true,
      }),
    )
  }

  /** Press, then a series of (dy, dt) moves straight down, then release. */
  function drag(target: Element, steps: Array<[number, number]>, release = true) {
    pointer('pointerdown', target, 100, 100)
    let y = 100
    for (const [dy, dt] of steps) {
      y += dy
      now += dt
      pointer('pointermove', target, 100, y)
    }
    if (release) pointer('pointerup', target, 100, y)
  }

  it('dismisses a long slow drag from the handle after gliding off from the release point', () => {
    drag(header, [[SLOP, 16], [100, 100], [100, 200], [10, 300]])
    expect(dismissed).toBe(0)
    expect(panel.style.transition).toContain('transform')
    expect(panel.style.transform).toContain('100%')
    expect(backdrop.style.opacity).toBe('0')
    expect(panel.hasAttribute(SHEET_DISMISSING_ATTR)).toBe(true)
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(1)
    // Reka's closed keyframes are silenced so Presence unmounts at once.
    expect(panel.style.animation).toBe('none')
    expect(backdrop.style.animation).toBe('none')
  })

  it('springs a short drag back and leaves no inline styles behind', () => {
    drag(header, [[SLOP, 16], [40, 100], [0, 300]])
    expect(dismissed).toBe(0)
    expect(panel.style.transition).toContain('transform')
    panel.dispatchEvent(new TransitionEvent('transitionend', { propertyName: 'transform' }))
    expect(panel.style.transform).toBe('')
    expect(panel.style.transition).toBe('')
    expect(panel.style.willChange).toBe('')
    expect(backdrop.style.opacity).toBe('')
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
  })

  it('dismisses a short fling', () => {
    // 60px in 50ms is 1.2 px/ms, far below the distance threshold.
    drag(header, [[SLOP, 16], [30, 25], [30, 25]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(1)
  })

  it('keeps a sheet dragged far down when the finger flicks back up', () => {
    drag(header, [[SLOP, 16], [300, 300], [-40, 20], [-40, 20]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
  })

  it('drags from the body when its list is at the top', () => {
    drag(body, [[SLOP, 16], [200, 200]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(1)
  })

  it('ignores a press in the body while its list is scrolled', () => {
    Object.defineProperty(list, 'scrollTop', { value: 120, configurable: true })
    drag(body, [[SLOP, 16], [300, 200]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
    expect(panel.style.transform).toBe('')
  })

  it('hands an upward move from the body back to scrolling', () => {
    drag(body, [[-SLOP, 16], [-100, 100], [250, 100]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
    expect(panel.style.willChange).toBe('')
  })

  it('never claims a horizontal drag', () => {
    pointer('pointerdown', header, 100, 100)
    now += 16
    pointer('pointermove', header, 130, 105)
    now += 100
    pointer('pointermove', header, 330, 400)
    pointer('pointerup', header, 330, 400)
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
    expect(panel.style.willChange).toBe('')
  })

  it('dismisses a downward drag over the picture of a video with native controls', () => {
    const video = document.createElement('video')
    video.controls = true
    video.getBoundingClientRect = () => ({ top: 0, bottom: 400 }) as DOMRect
    list.append(video)
    drag(video, [[SLOP, 16], [300, 200]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(1)
  })

  it('leaves a drag from a native video control bar to the platform', () => {
    const video = document.createElement('video')
    video.controls = true
    // The press at y=100 lands inside the bottom strip.
    video.getBoundingClientRect = () => ({ top: 0, bottom: 120 }) as DOMRect
    list.append(video)
    drag(video, [[SLOP, 16], [300, 200]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
    expect(panel.style.willChange).toBe('')
  })

  it('leaves a press on a field alone', () => {
    drag(panel.querySelector('#field')!, [[SLOP, 16], [300, 200]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
  })

  it('does nothing in the desktop presentation or with a fine pointer', () => {
    desktop = true
    drag(header, [[SLOP, 16], [300, 200]])
    desktop = false
    coarse = false
    drag(header, [[SLOP, 16], [300, 200]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
  })

  it('closes at once under reduced motion, with no glide', () => {
    reduced.value = true
    drag(header, [[SLOP, 16], [300, 200]])
    expect(dismissed).toBe(1)
    expect(panel.style.transition).toBe('')
    expect(panel.style.animation).toBe('none')
  })

  it('brings the sheet back if the consumer keeps it open', () => {
    drag(header, [[SLOP, 16], [300, 200]])
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(1)
    // The panel is still in the document: the consumer declined to close.
    vi.advanceTimersByTime(150)
    expect(panel.hasAttribute(SHEET_DISMISSING_ATTR)).toBe(true)
    expect(panel.style.transform).toBe('')
    panel.dispatchEvent(new TransitionEvent('transitionend', { propertyName: 'transform' }))
    expect(panel.hasAttribute(SHEET_DISMISSING_ATTR)).toBe(false)
    expect(panel.style.animation).toBe('')
  })

  it('clears everything when the panel unmounts mid-drag', async () => {
    drag(header, [[SLOP, 16], [100, 100]], false)
    panelRef.value = null
    await nextTick()
    expect(panel.style.willChange).toBe('')
    expect(panel.style.transform).toBe('')
    pointer('pointerup', header, 100, 210)
    vi.advanceTimersByTime(400)
    expect(dismissed).toBe(0)
  })
})
