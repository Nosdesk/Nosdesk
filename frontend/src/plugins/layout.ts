import { computed, type ComputedRef } from 'vue';
import type { PluginLayout } from '@nosdesk/plugin-sdk';
import { useMobileDetection, BREAKPOINTS } from '@/composables/useMobileDetection';

/**
 * The host layout facts pushed into a plugin sandbox.
 *
 * A sandboxed panel measures its own iframe, so it can see how wide IT is but
 * not what the app around it is doing. This supplies the missing half: the
 * app's active breakpoint, on the host's own `BREAKPOINTS` scale, so a plugin
 * branching on `md` means the same thing the app does.
 *
 * Only the breakpoint. Anything the guest can resolve for itself (pointer type,
 * colour scheme, reduced motion) is left to the guest: those media features are
 * device-level and evaluate correctly inside an iframe, so mirroring them here
 * would be a second source of truth that can only go stale.
 */

/** The app's active breakpoint for a viewport width, matching `BREAKPOINTS`. */
export function breakpointFor(width: number): PluginLayout['breakpoint'] {
  if (width >= BREAKPOINTS.xl) return 'xl';
  if (width >= BREAKPOINTS.lg) return 'lg';
  if (width >= BREAKPOINTS.md) return 'md';
  if (width >= BREAKPOINTS.sm) return 'sm';
  return 'base';
}

/**
 * Reactive host layout for the sandbox frames. Rides `useMobileDetection`'s
 * single shared, debounced resize listener rather than adding another one.
 *
 * Callers should watch `.value.breakpoint`, NOT the ref: the computed
 * re-evaluates on every debounced resize and hands back a fresh object each
 * time, so watching the object itself fires continuously through a drag even
 * though the bucket has not moved.
 */
export function usePluginLayout(): ComputedRef<PluginLayout> {
  const { windowWidth } = useMobileDetection();
  return computed(() => ({ breakpoint: breakpointFor(windowWidth.value) }));
}
