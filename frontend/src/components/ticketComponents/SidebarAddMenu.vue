<script setup lang="ts">
/**
 * SidebarAddMenu — unified "+ Add" dropdown for the ticket
 * sidebar. Native actions and plugin-contributed actions render
 * in two grouped sections under the same trigger button.
 *
 * Chrome (positioning, dismiss, focus, fade-scale transition,
 * gutter-aligned items, divider) lives in `<Popover>` +
 * `<MenuList>`. This file owns the trigger styling and the
 * native-vs-plugin item shaping.
 */
import { computed, ref } from 'vue'
import Popover from '@/components/common/Popover.vue'
import MenuList, { type MenuItem } from '@/components/common/MenuList.vue'

export interface SidebarAddMenuItem {
  id: string
  label: string
  type: 'native' | 'plugin'
  pluginName?: string
  /** SVG path for native icons (viewBox 0 0 24 24, stroke-based)
   * or image URL/data URI for plugins. */
  icon?: string
}

const props = defineProps<{
  items: SidebarAddMenuItem[]
}>()

const emit = defineEmits<{
  (e: 'select', itemId: string): void
}>()

const isOpen = ref(false)
const triggerRef = ref<HTMLElement | null>(null)

const anchor = computed(() => ({
  type: 'element' as const,
  element: () => triggerRef.value,
}))

/**
 * Map the input shape to MenuList's `MenuItem`. Native items
 * get their SVG path through `icon`; plugin items get a
 * runtime-supplied image through `iconUrl` (with a divider
 * preceding the first plugin item if natives exist above).
 * `pluginName` lands in `trailing` so the source of each
 * plugin contribution is visible without crowding the label.
 */
const menuItems = computed<MenuItem[]>(() => {
  const natives = props.items.filter((i) => i.type === 'native')
  const plugins = props.items.filter((i) => i.type === 'plugin')
  const out: MenuItem[] = natives.map((i) => ({
    id: i.id,
    label: i.label,
    icon: i.icon,
  }))
  plugins.forEach((i, idx) => {
    out.push({
      id: i.id,
      label: i.label,
      iconUrl: i.icon,
      trailing: i.pluginName,
      // Divider above the first plugin item if there are natives
      // above it; subsequent plugins flow without dividers.
      divider: idx === 0 && natives.length > 0,
    })
  })
  return out
})

function toggle() {
  isOpen.value = !isOpen.value
}

function handleSelect(id: string) {
  emit('select', id)
  isOpen.value = false
}
</script>

<template>
  <div class="relative print:hidden">
    <button
      ref="triggerRef"
      @click="toggle"
      class="group w-full py-2.5 px-4 rounded-xl border border-dashed border-default hover:border-accent/50 hover:bg-accent/5 transition-all duration-150 cursor-pointer"
    >
      <div
        class="flex items-center justify-center gap-2 text-sm text-tertiary group-hover:text-accent transition-colors"
      >
        <svg class="w-4 h-4" viewBox="0 0 20 20" fill="currentColor">
          <path
            d="M10 3a1 1 0 011 1v5h5a1 1 0 110 2h-5v5a1 1 0 11-2 0v-5H4a1 1 0 110-2h5V4a1 1 0 011-1z"
          />
        </svg>
        <span>Add</span>
        <svg
          class="w-3.5 h-3.5 transition-transform"
          :class="{ 'rotate-180': isOpen }"
          viewBox="0 0 20 20"
          fill="currentColor"
        >
          <path
            fill-rule="evenodd"
            d="M5.293 7.293a1 1 0 011.414 0L10 10.586l3.293-3.293a1 1 0 111.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 010-1.414z"
            clip-rule="evenodd"
          />
        </svg>
      </div>
    </button>

    <Popover
      :open="isOpen"
      :anchor="anchor"
      placement="bottom-start"
      react-to-scroll="reposition"
      :auto-focus="false"
      role="menu"
      popover-class="bg-surface border border-default rounded-lg shadow-lg py-1 min-w-[224px]"
      @close="isOpen = false"
    >
      <MenuList :items="menuItems" @select="handleSelect" />
    </Popover>
  </div>
</template>
