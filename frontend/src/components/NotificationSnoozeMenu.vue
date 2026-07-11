<script setup lang="ts">
/**
 * Per-row snooze menu for the notification inbox. Presents a few
 * presets ("Later today", "Tomorrow", "Next week"); each resolves to an
 * `until` instant (anchored to the user's timezone) and is emitted as an
 * ISO string. The parent row owns the mutation + list removal, mirroring
 * the archive action.
 *
 * Menu chrome (positioning, dismiss, focus, a11y) lives in
 * <ResponsiveMenu> + <MenuList>; this file is just the domain wiring,
 * modelled on <DocumentActionsMenu>.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { TZDate } from '@date-fns/tz'
import { addDays, addHours, set } from 'date-fns'
import { useDateStore } from '@nosdesk/core/stores/dateStore'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'
import { ICON_REGISTRY } from '@/components/common/icons'

// `title` is consumed directly in the template (trigger aria-label).
defineProps<{ title: string }>()
const emit = defineEmits<{ (e: 'snooze', until: string): void }>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string>) => fluent.$t(key, args)
const dateStore = useDateStore()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)

// Function form keeps the lookup live across re-mounts.
const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

function toggle() {
  isOpen.value = !isOpen.value
}
function closeMenu() {
  isOpen.value = false
}

const menuItems = computed<MenuItem[]>(() => [
  { id: 'later-today', label: t('inbox-snooze-later-today'), icon: ICON_REGISTRY.clock.d },
  { id: 'tomorrow', label: t('inbox-snooze-tomorrow'), icon: ICON_REGISTRY.clock.d },
  { id: 'next-week', label: t('inbox-snooze-next-week'), icon: ICON_REGISTRY.clock.d },
])

/**
 * Resolve a preset id to its `until` instant as an ISO-8601 string.
 * "Later today" is a pure +3h offset (no zone needed); the calendar
 * presets anchor 9am to the user's timezone via TZDate so "tomorrow"
 * means their morning, not the browser's.
 */
function untilFor(id: string): string | null {
  const tz = dateStore.effectiveTimezone
  const now = new Date()
  const at9am = (days: number) =>
    set(addDays(new TZDate(now, tz), days), {
      hours: 9,
      minutes: 0,
      seconds: 0,
      milliseconds: 0,
    }).toISOString()
  switch (id) {
    case 'later-today':
      return addHours(now, 3).toISOString()
    case 'tomorrow':
      return at9am(1)
    case 'next-week':
      return at9am(7)
    default:
      return null
  }
}

function handleSelect(id: string) {
  const until = untilFor(id)
  if (until) emit('snooze', until)
  closeMenu()
}
</script>

<template>
  <div class="relative">
    <button
      ref="triggerRef"
      type="button"
      class="rounded p-1 text-tertiary hover:bg-surface-alt hover:text-primary"
      :class="{ 'bg-surface-alt text-primary': isOpen }"
      :aria-label="t('inbox-snooze-trigger', { title })"
      @click.stop="toggle"
    >
      <Icon name="clock" size="xs" />
    </button>

    <ResponsiveMenu
      :open="isOpen"
      :anchor="anchor"
      :title="t('inbox-snooze-heading')"
      placement="bottom-end"
      react-to-scroll="reposition"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[180px]"
      @close="closeMenu"
    >
      <MenuList :items="menuItems" @select="handleSelect" />
    </ResponsiveMenu>
  </div>
</template>
