<script setup lang="ts">
import { ref, computed, onBeforeUnmount, nextTick, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import { useAssignmentPickerQueries } from '@/composables/useAssignmentPickerQueries'
import Icon from '@/components/common/Icon.vue'

const { $t } = useFluent()

export interface SelectedPrincipal {
  type: 'group' | 'user'
  id: string // group ID (as string) or user UUID
  name: string
  avatar?: string | null
}

const props = withDefaults(defineProps<{
  selectedItems: SelectedPrincipal[]
  placeholder?: string
}>(), {
  placeholder: 'Search users and groups...',
})

const emit = defineEmits<{
  (e: 'update:selectedItems', items: SelectedPrincipal[]): void
}>()

const searchQuery = ref('')
const showDropdown = ref(false)

const { allGroups, searchedUsers, loading } = useAssignmentPickerQueries(searchQuery)

// The dropdown uses the native HTML `popover` attribute + top-layer.
// Because top-layer paints above all stacking contexts (same layer
// `<dialog>.showModal()` renders in), we don't need `<Teleport>`,
// z-index fiddling, or any awareness of the surrounding modal. We
// still position it manually via `position: fixed` + the input's
// bounding rect, reapplied on scroll/resize so anchoring stays
// correct if the modal body scrolls.
const inputWrapperRef = ref<HTMLElement | null>(null)
const popoverRef = ref<HTMLElement | null>(null)
const dropdownPosition = ref({ left: 0, bottom: 0, width: 0 })

function updateDropdownPosition() {
  const el = inputWrapperRef.value
  if (!el) return
  const r = el.getBoundingClientRect()
  dropdownPosition.value = {
    left: r.left,
    width: r.width,
    // Opens above the input: `bottom` from viewport bottom equals
    // viewport height minus the input's top edge (plus a small gap).
    bottom: window.innerHeight - r.top + 4,
  }
}

const selectedSet = computed(() => {
  const set = new Set<string>()
  for (const item of props.selectedItems) {
    set.add(`${item.type}:${item.id}`)
  }
  return set
})

const filteredGroups = computed(() => {
  const q = searchQuery.value.toLowerCase()
  return allGroups.value.filter(g => {
    if (selectedSet.value.has(`group:${g.id}`)) return false
    if (!q) return true
    return g.name.toLowerCase().includes(q)
  })
})

const filteredUsers = computed(() =>
  searchedUsers.value.filter(u => !selectedSet.value.has(`user:${u.uuid}`)),
)

const hasResults = computed(() => filteredGroups.value.length > 0 || filteredUsers.value.length > 0)

const emptyStateKey = computed<string | null>(() => {
  if (hasResults.value || loading.value) return null
  if (searchQuery.value.trim()) return 'assignment-picker-no-results'
  if (props.selectedItems.length > 0) return 'assignment-picker-all-selected'
  return 'assignment-picker-empty-none'
})

const onSearchInput = () => {
  showDropdown.value = true
}

const addItem = (item: SelectedPrincipal) => {
  emit('update:selectedItems', [...props.selectedItems, item])
}

const removeItem = (item: SelectedPrincipal) => {
  emit('update:selectedItems', props.selectedItems.filter(i => !(i.type === item.type && i.id === item.id)))
}

const selectGroup = (group: { id: number; name: string }) => {
  addItem({
    type: 'group',
    id: String(group.id),
    name: group.name,
  })
}

const selectUser = (user: { uuid: string; name: string; avatar_url?: string | null }) => {
  addItem({
    type: 'user',
    id: user.uuid,
    name: user.name,
    avatar: user.avatar_url,
  })
}

const onFocus = () => {
  showDropdown.value = true
}

const onBlur = () => {
  // Small delay so click events on dropdown items fire first
  setTimeout(() => {
    showDropdown.value = false
  }, 200)
}

// Sync the native popover state with our `showDropdown` ref and keep
// the dropdown anchored to the input while it's open. Scroll listener
// is in capture phase so a modal body's scroll still triggers a
// reposition even if the handler stops propagation.
watch(showDropdown, async (open) => {
  if (open) {
    await nextTick()
    updateDropdownPosition()
    popoverRef.value?.showPopover?.()
    window.addEventListener('scroll', updateDropdownPosition, true)
    window.addEventListener('resize', updateDropdownPosition)
  } else {
    popoverRef.value?.hidePopover?.()
    window.removeEventListener('scroll', updateDropdownPosition, true)
    window.removeEventListener('resize', updateDropdownPosition)
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('scroll', updateDropdownPosition, true)
  window.removeEventListener('resize', updateDropdownPosition)
})
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Search input + dropdown -->
    <div ref="inputWrapperRef" class="relative">
      <input
        v-model="searchQuery"
        :placeholder="placeholder"
        @input="onSearchInput"
        @focus="onFocus"
        @blur="onBlur"
        class="w-full px-3 py-2 text-sm rounded-lg border border-default bg-surface text-primary placeholder:text-tertiary focus:outline-none focus:ring-1 focus:ring-accent/30 focus:border-accent/30"
      />
    </div>

    <!--
      Native `popover` element. The browser promotes it to the top
      layer (the same layer `<dialog>.showModal()` uses), which paints
      above every ancestor stacking context, so we don't need
      `<Teleport>`, a z-index token, or any knowledge of the
      surrounding modal. `popover="manual"` means we control
      show/hide ourselves; the @blur handler on the input closes it
      after a short delay so dropdown clicks still register.
    -->
    <div
      ref="popoverRef"
      popover="manual"
      class="assignment-picker-popover max-h-60 overflow-y-auto rounded-lg border border-default bg-surface shadow-lg"
      :style="{
        left: `${dropdownPosition.left}px`,
        width: `${dropdownPosition.width}px`,
        bottom: `${dropdownPosition.bottom}px`,
      }"
    >
        <div v-if="loading && !hasResults" class="px-3 py-4 text-xs text-tertiary text-center">
          {{ $t('assignment-picker-loading') }}
        </div>

        <div v-else-if="emptyStateKey" class="px-3 py-4 text-xs text-tertiary text-center">
          {{ $t(emptyStateKey) }}
        </div>

        <template v-else>
          <!-- Groups section -->
          <div v-if="filteredGroups.length > 0">
            <div class="px-3 py-1.5 text-[10px] font-semibold text-tertiary uppercase tracking-wider bg-surface-alt">
              {{ $t('assignment-picker-section-groups') }}
            </div>
            <button
              v-for="group in filteredGroups"
              :key="`g-${group.id}`"
              @mousedown.prevent="selectGroup(group)"
              class="w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-surface-hover transition-colors"
            >
              <Icon name="team" class="text-tertiary flex-shrink-0" />
              <span class="text-sm text-primary truncate">{{ group.name }}</span>
            </button>
          </div>

          <!-- Users section -->
          <div v-if="filteredUsers.length > 0">
            <div class="px-3 py-1.5 text-[10px] font-semibold text-tertiary uppercase tracking-wider bg-surface-alt">
              {{ $t('assignment-picker-section-users') }}
            </div>
            <button
              v-for="user in filteredUsers"
              :key="`u-${user.uuid}`"
              @mousedown.prevent="selectUser(user)"
              class="w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-surface-hover transition-colors"
            >
              <div class="w-5 h-5 rounded-full bg-accent/20 flex items-center justify-center flex-shrink-0 overflow-hidden">
                <img
                  v-if="user.avatar_url"
                  :src="user.avatar_url"
                  :alt="user.name"
                  class="w-full h-full object-cover"
                />
                <span v-else class="text-[10px] font-medium text-accent">{{ user.name.charAt(0).toUpperCase() }}</span>
              </div>
              <div class="flex-1 min-w-0">
                <div class="text-sm text-primary truncate">{{ user.name }}</div>
                <div class="text-[11px] text-tertiary truncate">{{ user.email }}</div>
              </div>
            </button>
          </div>
        </template>
    </div>

    <!-- Selected items as chips -->
    <div v-if="selectedItems.length > 0" class="flex flex-wrap gap-2">
      <span
        v-for="item in selectedItems"
        :key="`${item.type}:${item.id}`"
        class="inline-flex items-center gap-1.5 px-2 py-1 text-xs rounded-full border border-default bg-surface-alt text-primary"
      >
        <!-- Group icon -->
        <Icon v-if="item.type === 'group'" name="team" size="xs" class="text-tertiary flex-shrink-0" />
        <!-- User icon -->
        <svg v-else class="w-3 h-3 text-tertiary flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M16 7a4 4 0 11-8 0 4 4 0 018 0zM12 14a7 7 0 00-7 7h14a7 7 0 00-7-7z" />
        </svg>
        <span class="truncate max-w-[120px]">{{ item.name }}</span>
        <button
          @click="removeItem(item)"
          class="text-tertiary hover:text-primary transition-colors"
        >
          <Icon name="close" size="xs" />
        </button>
      </span>
    </div>
  </div>
</template>

<style scoped>
/*
  Reset the user-agent styling that browsers apply to any `[popover]`
  element — centered `margin: auto`, a default padding, and a solid
  border — so our Tailwind classes control the look. `position:
  fixed` keeps the anchoring math (left / width / bottom) working in
  the top layer.
*/
.assignment-picker-popover {
  position: fixed;
  margin: 0;
  padding: 0;
  border-width: 1px;
  inset: auto;
  min-width: 12rem;
}

.assignment-picker-popover:popover-open {
  display: block;
}
</style>
