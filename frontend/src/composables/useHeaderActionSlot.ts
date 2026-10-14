import { computed, onUnmounted, ref, toValue, watchEffect, type MaybeRefOrGetter } from 'vue'

/**
 * The app bar's trailing action slot on mobile.
 *
 * SiteHeader renders an empty span with this id just before the avatar. A
 * view that owns page-level actions (the documentation page's actions menu)
 * teleports its trigger into it and declares the slot filled; the header
 * then gives the avatar's room to the page action, the same trade iOS
 * detail screens make. The avatar's menu stays one back-tap away.
 *
 * Same shape as `useViewBackFallback`: one module ref, written by whichever
 * view is on screen, cleared when it unmounts.
 */
export const HEADER_ACTION_SLOT_ID = 'site-header-actions'

const filled = ref(false)

/** Read-only view for the header. */
export const headerActionSlotFilled = computed(() => filled.value)

/** Declare the slot filled for as long as `active` is true and the caller is mounted. */
export function useHeaderActionSlot(active: MaybeRefOrGetter<boolean>): void {
  watchEffect(() => {
    filled.value = toValue(active)
  })
  onUnmounted(() => {
    filled.value = false
  })
}
