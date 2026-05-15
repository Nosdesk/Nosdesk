<!--
  Combobox-style user picker for the ticket sidebar (and any other
  consumer that wants the "search/pick a user" affordance with a
  bounded eligible set).

  Architecturally:
    * `useUserPicker` owns search state, debounce, eligible-set
      loading, and the section data (Selected / You / Recent / All).
    * `useRecentUsers` owns the per-account LRU history persisted to
      localStorage, scoped per picker type.
    * This component is a presentational shell: input + dropdown
      chrome, keyboard navigation, and ARIA combobox wiring.

  Accessibility: implements the WAI-ARIA combobox pattern. Input
  carries role="combobox" with aria-expanded / aria-controls /
  aria-activedescendant; the dropdown is role="listbox"; section
  containers are role="group" with aria-labelledby; rows are
  role="option" with aria-selected.
-->
<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, useId, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useFluent } from 'fluent-vue'
import UserAvatar from '@/components/UserAvatar.vue'

const { $t } = useFluent()
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import ResponsivePanel from '@/components/common/ResponsivePanel.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useUserPicker, type PickerUser, type UserPickerType } from '@/composables/useUserPicker'

const props = defineProps<{
  modelValue: string
  placeholder?: string
  type: UserPickerType
  /** Optional pre-resolved row for the current selection. Lets the
   *  picker show the assignee/requester name on a fresh ticket page
   *  load without an extra round trip. */
  currentUser?: { uuid: string; name: string; email?: string; avatar_thumb?: string | null; avatar_url?: string | null } | null
  /** Hide the inline trailing clear-X. The ticket sidebar puts its
   *  own clear button outside the picker, so we suppress the inline
   *  one when both would render. */
  hideInlineClear?: boolean
}>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

defineExpose({
  focus: () => inputRef.value?.focus(),
  clear: () => commitSelection(''),
})

const router = useRouter()
const { isMobile } = useMobileDetection('md')

// Reactive refs we feed to the composable so it can re-evaluate
// `selected` / `currentUserRow` / `recent` whenever they change.
const selectedUuid = computed(() => props.modelValue)
const seedRef = computed(() => props.currentUser ?? null)

const picker = useUserPicker({
  type: props.type,
  selectedUuid,
  selectedUserSeed: seedRef,
})

// ---- Open / close state ----

const isOpen = ref(false)
const containerRef = ref<HTMLElement | null>(null)
const inputRef = ref<HTMLInputElement | null>(null)
const listboxRef = ref<HTMLElement | null>(null)

const listboxId = useId()

async function openDropdown() {
  if (isOpen.value) return
  isOpen.value = true
  await picker.loadEligible()
  await nextTick()
  // Default selection: prefer the currently-selected row if it's in
  // the dropdown; otherwise the first usable option. Skips group
  // headers since they aren't focusable.
  resetHighlight()
  scheduleReposition()
}

function closeDropdown() {
  if (!isOpen.value) return
  isOpen.value = false
  highlightedId.value = null
  picker.query.value = ''
}

// ---- Selection ----

function commitSelection(uuid: string, user?: PickerUser) {
  emit('update:modelValue', uuid)
  if (uuid && user) picker.remember(user)
  closeDropdown()
}

function selectUser(user: PickerUser) {
  commitSelection(user.uuid, user)
}

function navigateToProfile() {
  if (!props.modelValue) return
  router.push(`/admin/users/${props.modelValue}`)
}

// ---- Section flattening for keyboard navigation ----

// Each rendered option carries a stable id derived from its uuid +
// section name. The flattened array drives ↑/↓ navigation and the
// active-descendant ARIA wiring without coupling render order to
// keyboard order.
interface OptionRow {
  id: string
  user: PickerUser
  section: 'selected' | 'you' | 'recent' | 'results'
}

const optionRows = computed<OptionRow[]>(() => {
  const rows: OptionRow[] = []
  if (picker.selected.value && !picker.isFiltering.value) {
    rows.push({
      id: `${listboxId}-sel-${picker.selected.value.uuid}`,
      user: picker.selected.value,
      section: 'selected',
    })
  }
  if (picker.currentUserRow.value && !picker.isFiltering.value) {
    rows.push({
      id: `${listboxId}-you-${picker.currentUserRow.value.uuid}`,
      user: picker.currentUserRow.value,
      section: 'you',
    })
  }
  for (const r of picker.recent.value) {
    rows.push({ id: `${listboxId}-rec-${r.uuid}`, user: r, section: 'recent' })
  }
  for (const r of picker.results.value) {
    rows.push({ id: `${listboxId}-all-${r.uuid}`, user: r, section: 'results' })
  }
  return rows
})

const highlightedId = ref<string | null>(null)
const highlightedIndex = computed(() =>
  optionRows.value.findIndex((r) => r.id === highlightedId.value),
)

function resetHighlight() {
  // Default to the current selection if visible, else first row.
  if (picker.selected.value && !picker.isFiltering.value) {
    highlightedId.value = `${listboxId}-sel-${picker.selected.value.uuid}`
    return
  }
  highlightedId.value = optionRows.value[0]?.id ?? null
}

function moveHighlight(delta: number) {
  const rows = optionRows.value
  if (rows.length === 0) {
    highlightedId.value = null
    return
  }
  const current = highlightedIndex.value
  const next = current < 0 ? (delta > 0 ? 0 : rows.length - 1) : (current + delta + rows.length) % rows.length
  highlightedId.value = rows[next].id
  scrollIntoView()
}

function jumpToEdge(direction: 'home' | 'end') {
  const rows = optionRows.value
  if (rows.length === 0) return
  highlightedId.value = rows[direction === 'home' ? 0 : rows.length - 1].id
  scrollIntoView()
}

function scrollIntoView() {
  nextTick(() => {
    const el = listboxRef.value?.querySelector<HTMLElement>(`#${CSS.escape(highlightedId.value ?? '')}`)
    el?.scrollIntoView({ block: 'nearest' })
  })
}

watch(optionRows, (rows) => {
  // If the highlighted row disappears (filter changed), default to
  // the first remaining row instead of stranding focus.
  if (!highlightedId.value) return
  if (!rows.some((r) => r.id === highlightedId.value)) {
    highlightedId.value = rows[0]?.id ?? null
  }
})

// ---- Keyboard handling ----

function onKeydown(event: KeyboardEvent) {
  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      if (!isOpen.value) {
        openDropdown()
      } else {
        moveHighlight(1)
      }
      break
    case 'ArrowUp':
      event.preventDefault()
      if (!isOpen.value) {
        openDropdown()
      } else {
        moveHighlight(-1)
      }
      break
    case 'Home':
      if (isOpen.value) {
        event.preventDefault()
        jumpToEdge('home')
      }
      break
    case 'End':
      if (isOpen.value) {
        event.preventDefault()
        jumpToEdge('end')
      }
      break
    case 'Enter': {
      event.preventDefault()
      const row = optionRows.value.find((r) => r.id === highlightedId.value)
      if (row) selectUser(row.user)
      break
    }
    case 'Escape':
      if (isOpen.value) {
        event.preventDefault()
        closeDropdown()
      }
      break
    case 'Tab':
      // Standard combobox behaviour: Tab with a highlighted option
      // confirms the selection (per WAI-ARIA APG). Without highlight,
      // Tab just dismisses.
      if (isOpen.value) {
        const row = optionRows.value.find((r) => r.id === highlightedId.value)
        if (row && row.user.uuid !== props.modelValue) {
          selectUser(row.user)
        } else {
          closeDropdown()
        }
      }
      break
  }
}

// ---- Click-outside dismiss (desktop) ----

function onDocumentClick(event: MouseEvent) {
  if (!isOpen.value || isMobile.value) return
  const target = event.target as Node
  if (containerRef.value?.contains(target)) return
  if (listboxRef.value?.contains(target)) return
  closeDropdown()
}

onMounted(() => document.addEventListener('mousedown', onDocumentClick))
onUnmounted(() => document.removeEventListener('mousedown', onDocumentClick))

// ---- Desktop dropdown positioning (fixed below/above input) ----

const menuStyle = ref({ top: '0px', left: '0px', width: '0px', maxHeight: '320px' })

function reposition() {
  if (!containerRef.value || isMobile.value) return
  const rect = containerRef.value.getBoundingClientRect()
  const viewportH = window.innerHeight
  const desiredMax = 360
  const spaceBelow = viewportH - rect.bottom
  const spaceAbove = rect.top
  const openUpward = spaceBelow < 240 && spaceAbove > spaceBelow
  const maxHeight = Math.min(desiredMax, openUpward ? spaceAbove - 16 : spaceBelow - 16)
  const top = openUpward ? rect.top - 4 - maxHeight : rect.bottom + 4
  menuStyle.value = {
    top: `${Math.max(8, top)}px`,
    left: `${rect.left}px`,
    width: `${Math.max(rect.width, 280)}px`,
    maxHeight: `${maxHeight}px`,
  }
}

let scrollHandler: (() => void) | null = null
function scheduleReposition() {
  reposition()
  if (!scrollHandler) {
    scrollHandler = () => isOpen.value && !isMobile.value && reposition()
    window.addEventListener('scroll', scrollHandler, true)
    window.addEventListener('resize', scrollHandler)
  }
}

onUnmounted(() => {
  if (scrollHandler) {
    window.removeEventListener('scroll', scrollHandler, true)
    window.removeEventListener('resize', scrollHandler)
  }
})

// ---- Display helpers ----

const inputDisplay = computed({
  get: () => (isOpen.value ? picker.query.value : picker.selectedDisplayName.value),
  set: (v: string) => {
    picker.query.value = v
  },
})

function onFocus() {
  openDropdown()
  // Select the input contents so typing replaces the selected name
  // rather than appending to it.
  setTimeout(() => inputRef.value?.select(), 0)
}

function emptyHint(): string {
  if (picker.isLoading.value) return ''
  if (!picker.isFiltering.value) {
    if (optionRows.value.length === 0) {
      return props.type === 'assignee'
        ? $t('ticket-picker-user-empty-assignees')
        : $t('ticket-picker-user-empty-users')
    }
    return ''
  }
  return $t('ticket-picker-user-empty-search', { query: picker.query.value.trim() })
}

function sectionLabel(section: OptionRow['section']): string {
  switch (section) {
    case 'selected':
      return props.type === 'assignee'
        ? $t('ticket-picker-user-section-selected-assignee')
        : $t('ticket-picker-user-section-selected-requester')
    case 'you':
      return $t('ticket-picker-user-section-you')
    case 'recent':
      return $t('ticket-picker-user-section-recent')
    case 'results':
      return picker.isFiltering.value
        ? $t('ticket-picker-user-section-results')
        : props.type === 'assignee'
          ? $t('ticket-picker-user-section-staff')
          : $t('ticket-picker-user-section-all')
  }
}

// Visible section markers — used to render group headers above the
// first row of each section. We compute "first row per section" so
// the template can place a `<li role="presentation">` label without
// inflating the row index.
const sectionStarts = computed(() => {
  const seen = new Set<OptionRow['section']>()
  const starts: Record<string, boolean> = {}
  for (const row of optionRows.value) {
    if (!seen.has(row.section)) {
      starts[row.id] = true
      seen.add(row.section)
    }
  }
  return starts
})
</script>

<template>
  <div ref="containerRef" class="relative w-full">
    <!-- Trigger row: avatar + input + clear -->
    <div
      class="flex items-center gap-2 sm:gap-2.5 px-2.5 sm:px-3 min-h-[44px] sm:min-h-[40px] cursor-text"
      @click="inputRef?.focus()"
    >
      <div class="flex-shrink-0 w-7 h-7 sm:w-6 sm:h-6 flex items-center justify-center">
        <button
          v-if="modelValue && picker.selectedDisplayName.value && !isOpen"
          type="button"
          @click.stop="navigateToProfile"
          class="rounded-full hover:ring-2 hover:ring-accent/50 transition-all cursor-pointer"
          :title="$t('ticket-picker-user-view-profile', { name: picker.selectedDisplayName.value })"
        >
          <UserAvatar
            :name="modelValue"
            :userName="picker.selected.value?.name"
            :avatar="picker.selected.value?.avatar_thumb || picker.selected.value?.avatar_url || null"
            :showName="false"
            size="sm"
            :clickable="false"
          />
        </button>
        <div
          v-else
          class="w-7 h-7 sm:w-6 sm:h-6 rounded-full bg-surface border border-subtle flex items-center justify-center transition-colors"
          :class="{ 'border-accent/50 bg-accent/5': isOpen }"
        >
          <Icon name="user" size="xs" class="text-tertiary" />
        </div>
      </div>

      <div class="flex-1 min-w-0">
        <input
          ref="inputRef"
          v-model="inputDisplay"
          type="text"
          role="combobox"
          :aria-expanded="isOpen"
          :aria-controls="listboxId"
          :aria-activedescendant="highlightedId ?? undefined"
          aria-autocomplete="list"
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
          :placeholder="placeholder || (type === 'assignee' ? $t('ticket-picker-user-placeholder-assignee') : $t('ticket-picker-user-placeholder-requester'))"
          class="w-full bg-transparent text-secondary placeholder-tertiary focus:outline-none text-sm leading-tight py-1"
          @focus="onFocus"
          @keydown="onKeydown"
        />
      </div>

      <div class="flex items-center gap-1.5 flex-shrink-0">
        <span v-if="picker.isLoading.value" class="text-tertiary inline-flex">
          <Spinner size="xs" :label="type === 'assignee' ? $t('ticket-picker-user-loading-assignee') : $t('ticket-picker-user-loading-requester')" />
        </span>
        <button
          v-if="!hideInlineClear && modelValue && !isOpen"
          type="button"
          class="p-1 rounded-full text-tertiary hover:text-secondary hover:bg-surface-hover transition-colors"
          :aria-label="$t('ticket-picker-user-clear')"
          :title="$t('ticket-picker-user-clear')"
          @click.stop="commitSelection('')"
        >
          <Icon name="close" size="xs" />
        </button>
      </div>
    </div>

    <!-- Desktop dropdown — teleport to body so the fixed-positioned
         menu isn't clipped by the SectionCard's overflow-hidden chrome. -->
    <Teleport to="body">
      <Transition name="user-picker">
        <div
          v-if="isOpen && !isMobile"
          ref="listboxRef"
          class="user-picker-menu fixed z-overlay rounded-lg border border-default bg-surface shadow-lg shadow-black/10 dark:shadow-black/30"
          :style="menuStyle"
        >
          <div class="flex flex-col h-full max-h-[inherit] overflow-hidden">
            <ul
              :id="listboxId"
              role="listbox"
              :aria-label="type === 'assignee' ? $t('ticket-picker-user-listbox-assignees') : $t('ticket-picker-user-listbox-users')"
              class="flex-1 overflow-y-auto py-1 outline-none"
              tabindex="-1"
            >
              <template v-for="row in optionRows" :key="row.id">
                <!-- Group header rendered before the first row of each section. -->
                <li
                  v-if="sectionStarts[row.id]"
                  role="presentation"
                  class="px-3 pt-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-tertiary select-none"
                >
                  {{ sectionLabel(row.section) }}
                </li>
                <li
                  :id="row.id"
                  role="option"
                  :aria-selected="row.user.uuid === modelValue"
                  :data-section="row.section"
                  class="mx-1 px-2 py-1.5 rounded-md flex items-center gap-2.5 cursor-pointer transition-colors"
                  :class="
                    row.id === highlightedId
                      ? 'bg-accent/10'
                      : 'hover:bg-surface-hover/60'
                  "
                  @mouseenter="highlightedId = row.id"
                  @mousedown.prevent
                  @click="selectUser(row.user)"
                >
                  <UserAvatar
                    :name="row.user.uuid"
                    :userName="row.user.name"
                    :avatar="row.user.avatar_thumb || row.user.avatar_url || null"
                    :showName="false"
                    size="xs"
                    :clickable="false"
                  />
                  <div class="flex-1 min-w-0">
                    <div class="text-[13px] text-primary truncate">
                      {{ row.user.name
                      }}<span v-if="row.section === 'you'" class="text-tertiary font-normal"> {{ $t('ticket-picker-user-you-suffix') }}</span>
                    </div>
                    <div v-if="row.user.email" class="text-[11px] text-tertiary truncate">
                      {{ row.user.email }}
                    </div>
                  </div>
                  <Icon
                    v-if="row.user.uuid === modelValue"
                    name="check"
                    size="xs"
                    class="text-accent flex-shrink-0"
                  />
                </li>
              </template>

              <li
                v-if="optionRows.length === 0 && !picker.isLoading.value"
                role="presentation"
                class="px-3 py-6 text-center text-[12px] text-tertiary"
              >
                {{ emptyHint() }}
              </li>

              <li
                v-if="picker.isLoading.value && optionRows.length === 0"
                role="presentation"
                class="px-3 py-6 flex items-center justify-center"
              >
                <Spinner size="sm" />
              </li>
            </ul>
          </div>
        </div>
      </Transition>
    </Teleport>

    <!-- Mobile bottom sheet — same option list, framed by ResponsivePanel. -->
    <ResponsivePanel
      v-if="isMobile"
      :open="isOpen"
      :title="type === 'assignee' ? $t('ticket-picker-user-sheet-title-assignee') : $t('ticket-picker-user-sheet-title-requester')"
      side-panel-class="w-80"
      @close="closeDropdown"
    >
      <div class="px-3 pt-2 pb-1 border-b border-default">
        <input
          v-model="picker.query.value"
          type="text"
          autocomplete="off"
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
          :placeholder="type === 'assignee' ? $t('ticket-picker-user-search-staff') : $t('ticket-picker-user-search-users')"
          class="w-full px-3 py-2 rounded-md border border-default bg-surface-alt text-sm text-primary placeholder-tertiary focus:border-accent focus:ring-1 focus:ring-accent/30 focus:outline-none"
          @keydown="onKeydown"
        />
      </div>
      <ul
        :id="`${listboxId}-mobile`"
        role="listbox"
        :aria-label="type === 'assignee' ? 'Assignable users' : 'Users'"
        class="flex-1 overflow-y-auto py-1"
      >
        <template v-for="row in optionRows" :key="`m-${row.id}`">
          <li
            v-if="sectionStarts[row.id]"
            role="presentation"
            class="px-4 pt-3 pb-1 text-[10px] font-semibold uppercase tracking-wider text-tertiary select-none"
          >
            {{ sectionLabel(row.section) }}
          </li>
          <li
            :id="`m-${row.id}`"
            role="option"
            :aria-selected="row.user.uuid === modelValue"
            class="px-3 py-2.5 flex items-center gap-3 cursor-pointer hover:bg-surface-hover/60 active:bg-surface-alt transition-colors"
            @click="selectUser(row.user)"
          >
            <UserAvatar
              :name="row.user.uuid"
              :userName="row.user.name"
              :avatar="row.user.avatar_thumb || row.user.avatar_url || null"
              :showName="false"
              size="sm"
              :clickable="false"
            />
            <div class="flex-1 min-w-0">
              <div class="text-sm text-primary truncate">
                {{ row.user.name
                }}<span v-if="row.section === 'you'" class="text-tertiary font-normal"> (you)</span>
              </div>
              <div v-if="row.user.email" class="text-xs text-tertiary truncate">
                {{ row.user.email }}
              </div>
            </div>
            <Icon
              v-if="row.user.uuid === modelValue"
              name="check"
              size="sm"
              class="text-accent flex-shrink-0"
            />
          </li>
        </template>
        <li
          v-if="optionRows.length === 0 && !picker.isLoading.value"
          role="presentation"
          class="px-4 py-8 text-center text-sm text-tertiary"
        >
          {{ emptyHint() }}
        </li>
        <li
          v-if="picker.isLoading.value && optionRows.length === 0"
          role="presentation"
          class="px-3 py-8 flex items-center justify-center"
        >
          <Spinner size="md" />
        </li>
      </ul>
    </ResponsivePanel>
  </div>
</template>

<style scoped>
.user-picker-enter-active,
.user-picker-leave-active {
  transition: opacity 120ms ease, transform 120ms ease;
}
.user-picker-enter-from,
.user-picker-leave-to {
  opacity: 0;
  transform: scale(0.98) translateY(-2px);
}
</style>
