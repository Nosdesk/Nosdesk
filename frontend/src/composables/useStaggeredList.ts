/**
 * Provides stagger delay styles for list item animations.
 * Caps the stagger delay so later items don't have excessive delays.
 */
export function useStaggeredList(options: {
  staggerDelay?: number
  animationDuration?: number
  maxStaggerItems?: number
} = {}) {
  const {
    staggerDelay = 20,
    animationDuration = 150,
    maxStaggerItems = 10
  } = options

  const getStyle = (index: number): Record<string, string> => ({
    '--stagger-delay': `${Math.min(index, maxStaggerItems - 1) * staggerDelay}ms`,
    '--animation-duration': `${animationDuration}ms`
  })

  return { getStyle }
}
