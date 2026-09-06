<script setup lang="ts">
/**
 * Dataset-agnostic group-by picker. Sits in a list view's filter
 * row next to the chip strip; opens a small popover with axis
 * options (always including "None"). Pair with `useListGrouping`
 * — feed its `axisOptions` in and bind `groupBy` / `setGroupBy`.
 *
 * The trigger reads as a quiet outline button when no axis is
 * selected ("Group by"), and shifts to the accent-active style
 * with the current axis label when grouping is on. Same visual
 * vocabulary as the filter chips so the two affordances live in
 * the same toolbar without competing.
 */
import { computed, ref } from 'vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import { useMenuKeyboardNav, type KeyboardNavItem } from '@/composables/useMenuKeyboardNav'
import type { PopoverAnchor } from '@/composables/usePopover'
import type { GroupOption } from '@/composables/useListGrouping'
import { NONE_AXIS_KEY } from '@/composables/useListGrouping'

const props = defineProps<{
  options: GroupOption[]
  modelValue: string
  /** Label for the trigger when no axis is selected. Defaults to
   *  the generic "Group by" string. */
  triggerLabel?: string
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)
const listRef = ref<HTMLDivElement | null>(null)

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}))

const activeOption = computed<GroupOption | undefined>(() =>
  props.options.find((o) => o.key === props.modelValue),
)
const isActive = computed<boolean>(() => props.modelValue !== NONE_AXIS_KEY)

interface NavItem extends KeyboardNavItem {
  option: GroupOption
}

const nav = useMenuKeyboardNav<NavItem>((item) => pick(item.option.key))

const navItems = computed<NavItem[]>(() =>
  props.options.map((o) => ({ label: o.label, option: o })),
)

function pick(key: string): void {
  emit('update:modelValue', key)
  open.value = false
}
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      :class="[
        'inline-flex items-center gap-1 text-2xs px-2 h-6 rounded-md border transition-colors',
        isActive
          ? 'border-accent/40 bg-accent/10 text-accent'
          : 'text-tertiary hover:text-primary border-subtle hover:border-default hover:bg-surface-hover',
      ]"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <span v-if="!isActive">{{ triggerLabel ?? $t('list-grouping-trigger') }}</span>
      <span v-else class="inline-flex items-center gap-1">
        <span class="text-tertiary">{{ $t('list-grouping-trigger') }}:</span>
        <span>{{ activeOption?.label }}</span>
      </span>
      <Icon name="chevronDown" class="w-3 h-3 opacity-70" />
    </button>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      :title="$t('list-grouping-trigger')"
      placement="bottom-start"
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[12rem] max-w-[18rem]"
      @close="open = false"
    >
      <div
        ref="listRef"
        tabindex="-1"
        role="menu"
        class="py-1 outline-none"
        @keydown="(e) => { nav.setItems(navItems); nav.onKeydown(e) }"
      >
        <button
          v-for="(opt, i) in options"
          :key="opt.key"
          type="button"
          role="menuitem"
          class="w-full px-3 py-1.5 grid grid-cols-[1fr_auto] items-center gap-x-2 text-left transition-colors duration-75"
          :class="nav.highlightedIndex.value === i
            ? 'bg-accent/10'
            : 'hover:bg-surface-hover'"
          @click.stop="pick(opt.key)"
          @mouseenter="nav.setHighlighted(i)"
        >
          <span class="text-xs text-primary">{{ opt.label }}</span>
          <Icon
            v-if="opt.key === modelValue"
            name="check"
            class="w-3 h-3 text-accent"
          />
        </button>
      </div>
    </ResponsiveMenu>
  </div>
</template>
