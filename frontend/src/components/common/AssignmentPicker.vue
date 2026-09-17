<script setup lang="ts">
/**
 * Multi-principal picker (users and groups) with server-side search, on
 * Reka's Combobox. Reka owns the combobox ARIA (`role=combobox` input
 * with `aria-expanded`/`aria-controls`/`aria-activedescendant`, grouped
 * `role=option` rows), the arrow/Home/End/Enter/Escape model, the
 * positioned popup (above the input, flipping when there is no room)
 * and the dismiss layer, which is how it stays correct inside a modal.
 * Filtering is server-side (`ignoreFilter`); a pick adds a chip and drops
 * the row from the list, and the list stays open for the next pick.
 */
import { ref, computed } from 'vue'
import { useFluent } from 'fluent-vue'
import {
  ComboboxAnchor,
  ComboboxContent,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxLabel,
  ComboboxPortal,
  ComboboxRoot,
  ComboboxViewport,
} from 'reka-ui'
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
const isOpen = ref(false)
const { allGroups, searchedUsers, loading } = useAssignmentPickerQueries(searchQuery)

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

const addItem = (item: SelectedPrincipal) => {
  emit('update:selectedItems', [...props.selectedItems, item])
}

const removeItem = (item: SelectedPrincipal) => {
  emit('update:selectedItems', props.selectedItems.filter(i => !(i.type === item.type && i.id === item.id)))
}

// Item values are composite keys; the combobox model is never retained
// (a pick becomes a chip and leaves the list), so every update carries
// exactly the key that was just chosen.
function onPick(next: unknown) {
  const keys = Array.isArray(next) ? (next as string[]) : []
  const key = keys[keys.length - 1]
  if (!key) return
  const [type, id] = [key.slice(0, key.indexOf(':')), key.slice(key.indexOf(':') + 1)]
  if (type === 'group') {
    const group = allGroups.value.find(g => String(g.id) === id)
    if (group) addItem({ type: 'group', id: String(group.id), name: group.name })
  } else {
    const user = searchedUsers.value.find(u => u.uuid === id)
    if (user) addItem({ type: 'user', id: user.uuid, name: user.name, avatar: user.avatar_url })
  }
}

const rowClass =
  'w-full flex items-center gap-3 px-3 py-2.5 min-h-[44px] md:min-h-0 text-left hover:bg-surface-hover data-[highlighted]:bg-surface-hover transition-colors outline-none cursor-default'
</script>

<template>
  <div class="flex flex-col gap-2">
    <ComboboxRoot
      v-model:open="isOpen"
      :model-value="[]"
      multiple
      ignore-filter
      open-on-focus
      open-on-click
      highlight-on-hover
      :reset-search-term-on-select="false"
      :reset-search-term-on-blur="false"
      @update:model-value="onPick"
    >
      <ComboboxAnchor class="relative">
        <ComboboxInput
          v-model="searchQuery"
          :placeholder="placeholder"
          :aria-label="placeholder"
          class="w-full px-3 py-2 text-sm rounded-lg border border-default bg-surface text-primary placeholder:text-tertiary focus:outline-none focus:ring-1 focus:ring-accent/30 focus:border-accent/30"
        />
      </ComboboxAnchor>

      <ComboboxPortal>
        <ComboboxContent
          position="popper"
          side="top"
          align="start"
          :side-offset="4"
          :collision-padding="8"
          class="popover-inner assignment-picker-surface z-overlay max-h-60 overflow-y-auto rounded-lg border border-default bg-surface shadow-lg"
          :style="{ width: 'var(--reka-combobox-trigger-width)', minWidth: '12rem' }"
        >
          <ComboboxViewport>
            <div v-if="loading && !hasResults" class="px-3 py-4 text-xs text-tertiary text-center">
              {{ $t('assignment-picker-loading') }}
            </div>

            <div v-else-if="emptyStateKey" class="px-3 py-4 text-xs text-tertiary text-center">
              {{ $t(emptyStateKey) }}
            </div>

            <template v-else>
              <ComboboxGroup v-if="filteredGroups.length > 0">
                <ComboboxLabel class="block px-3 py-1.5 text-3xs font-semibold text-tertiary uppercase tracking-wider bg-surface-alt">
                  {{ $t('assignment-picker-section-groups') }}
                </ComboboxLabel>
                <ComboboxItem
                  v-for="group in filteredGroups"
                  :key="`g-${group.id}`"
                  :value="`group:${group.id}`"
                  :text-value="group.name"
                  :class="rowClass"
                >
                  <Icon name="team" class="text-tertiary flex-shrink-0" />
                  <span class="text-sm text-primary truncate">{{ group.name }}</span>
                </ComboboxItem>
              </ComboboxGroup>

              <ComboboxGroup v-if="filteredUsers.length > 0">
                <ComboboxLabel class="block px-3 py-1.5 text-3xs font-semibold text-tertiary uppercase tracking-wider bg-surface-alt">
                  {{ $t('assignment-picker-section-users') }}
                </ComboboxLabel>
                <ComboboxItem
                  v-for="user in filteredUsers"
                  :key="`u-${user.uuid}`"
                  :value="`user:${user.uuid}`"
                  :text-value="user.name"
                  :class="rowClass"
                >
                  <div class="w-5 h-5 rounded-full bg-accent/20 flex items-center justify-center flex-shrink-0 overflow-hidden">
                    <img
                      v-if="user.avatar_url"
                      :src="user.avatar_url"
                      :alt="user.name"
                      class="w-full h-full object-cover"
                    />
                    <span v-else class="text-3xs font-medium text-accent">{{ user.name.charAt(0).toUpperCase() }}</span>
                  </div>
                  <div class="flex-1 min-w-0">
                    <div class="text-sm text-primary truncate">{{ user.name }}</div>
                    <div class="text-2xs text-tertiary truncate">{{ user.email }}</div>
                  </div>
                </ComboboxItem>
              </ComboboxGroup>
            </template>
          </ComboboxViewport>
        </ComboboxContent>
      </ComboboxPortal>
    </ComboboxRoot>

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
          type="button"
          @click="removeItem(item)"
          class="text-tertiary hover:text-primary transition-colors"
          :aria-label="$t('common-remove-item-aria', { name: item.name })"
        >
          <Icon name="close" size="xs" />
        </button>
      </span>
    </div>
  </div>
</template>

<style>
.assignment-picker-surface {
  transform-origin: var(--reka-combobox-content-transform-origin);
}
</style>
