<!--
  Combobox-style user picker for the ticket sidebar (and any other
  consumer that wants the "search/pick a user" affordance with a
  bounded eligible set).

  Architecturally:
    * `useUserPicker` owns search state, debounce, eligible-set
      loading, and the section data (Selected / You / Recent / All).
    * `useRecentUsers` owns the per-account LRU history persisted to
      localStorage, scoped per picker type.
    * This component is a presentational shell on Reka's Combobox,
      which owns the combobox ARIA (`role=combobox` input with
      `aria-expanded` / `aria-controls` / `aria-activedescendant`,
      grouped `role=option` rows), the arrow / Home / End / Enter /
      Escape model, the positioned popup and the dismiss layer.

  Below `md` the trigger is a button and the list opens in a bottom
  sheet with its own search input; the same Combobox root drives it,
  its content rendered inline in the sheet.
-->
<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue'
import { useRouter } from 'vue-router'
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
import UserAvatar from '@/components/UserAvatar.vue'
import Icon from '@/components/common/Icon.vue'
import Spinner from '@/components/common/Spinner.vue'
import ResponsivePanel from '@/components/common/ResponsivePanel.vue'
import { useMobileDetection } from '@/composables/useMobileDetection'
import { useUserPicker, type PickerUser, type UserPickerType } from '@/composables/useUserPicker'

const { $t } = useFluent()

const props = withDefaults(defineProps<{
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
  /** Dense trigger for split-view preview property rows — matches
   *  `UserCell` typography (11px name, xxs avatar). */
  compact?: boolean
}>(), {
  compact: false,
})

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

defineExpose({
  focus: () => inputEl()?.focus(),
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
const rootRef = ref<{ highlightSelected?: () => Promise<void> } | null>(null)
const inputRef = ref<{ $el?: HTMLInputElement } | null>(null)
const inputEl = () => inputRef.value?.$el ?? null

// What the input shows: the query while the list is open (it starts
// blank so the placeholder invites typing), the selected name once it
// closes (Reka resets it to `displayValue`).
const inputText = ref('')
watch(inputText, (text) => {
  if (isOpen.value) picker.query.value = text
})

// The sheet is portalled, so its input and rows sit outside the
// combobox root in the DOM; Reka would read focus or a tap there as
// "outside" and close. The sheet owns its own dismissal.
const keepOpen = (event: Event) => event.preventDefault()

// Reka hands the input its `aria-controls` id when the content mounts,
// once, so in the sheet the input waits for the list.
const sheetListMounted = ref(false)

async function onOpenChange(open: boolean) {
  isOpen.value = open
  if (open) {
    // Focus leaving the root (the sheet button to the sheet's input)
    // would read as leaving the list and drop the highlight.
    if (isMobile.value) (document.activeElement as HTMLElement | null)?.blur()
    inputText.value = ''
    picker.query.value = ''
    await picker.loadEligible()
    // The list may have opened empty (or from the sheet button, which
    // skips Reka's own open path): point the highlight at the current
    // user, or the first row.
    await rootRef.value?.highlightSelected?.()
  } else {
    picker.query.value = ''
  }
}

// ---- Selection ----

function commitSelection(uuid: string, user?: PickerUser) {
  emit('update:modelValue', uuid)
  if (uuid && user) picker.remember(user)
  if (isOpen.value) onOpenChange(false)
}

// Every option carries the user's uuid; the same person can sit in
// more than one section, so the pick resolves to whichever row holds
// that uuid.
function onPick(value: unknown) {
  if (typeof value !== 'string' || !value) return
  const user = sections.value.flatMap((s) => s.rows).find((u) => u.uuid === value)
  commitSelection(value, user)
}

function navigateToProfile() {
  if (!props.modelValue) return
  router.push(`/users/${props.modelValue}`)
}

// ---- Sections ----

type Section = 'selected' | 'you' | 'recent' | 'results'

const sections = computed<{ key: Section; rows: PickerUser[] }[]>(() => {
  const out: { key: Section; rows: PickerUser[] }[] = []
  if (picker.selected.value && !picker.isFiltering.value) out.push({ key: 'selected', rows: [picker.selected.value] })
  if (picker.currentUserRow.value && !picker.isFiltering.value) out.push({ key: 'you', rows: [picker.currentUserRow.value] })
  if (picker.recent.value.length) out.push({ key: 'recent', rows: picker.recent.value })
  if (picker.results.value.length) out.push({ key: 'results', rows: picker.results.value })
  return out
})
const hasRows = computed(() => sections.value.length > 0)

function emptyHint(): string {
  if (picker.isLoading.value) return ''
  if (!picker.isFiltering.value) {
    if (!hasRows.value) {
      return props.type === 'assignee'
        ? $t('ticket-picker-user-empty-assignees')
        : $t('ticket-picker-user-empty-users')
    }
    return ''
  }
  return $t('ticket-picker-user-empty-search', { query: picker.query.value.trim() })
}

function sectionLabel(section: Section): string {
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

// Reka reads a group's label id once, before the label has mounted,
// so the pair is wired by hand.
const uid = useId()
const labelId = (section: Section) => `user-picker-${uid}-${section}`

const listLabel = computed(() =>
  props.type === 'assignee' ? $t('ticket-picker-user-listbox-assignees') : $t('ticket-picker-user-listbox-users'),
)
const inputPlaceholder = computed(
  () =>
    props.placeholder ||
    (props.type === 'assignee' ? $t('ticket-picker-user-placeholder-assignee') : $t('ticket-picker-user-placeholder-requester')),
)
</script>

<template>
  <!-- `selection-behavior` reaches the listbox under the combobox:
       re-picking the current user is still a pick, not a deselect. -->
  <ComboboxRoot
    ref="rootRef"
    class="relative w-full"
    :open="isOpen"
    :model-value="modelValue"
    ignore-filter
    :open-on-focus="!isMobile"
    :open-on-click="!isMobile"
    highlight-on-hover
    selection-behavior="replace"
    @update:open="onOpenChange"
    @update:model-value="onPick"
  >
    <!-- Trigger row: avatar + input + clear. Desktop sizing is the
         compact-row scale used by the sidebar's property panel
         (32px row + 20px avatar); mobile keeps the WCAG 2.5.8
         touch-target floor (44px row + 28px avatar). -->
    <ComboboxAnchor as-child>
      <div
        class="flex items-center cursor-text"
        :class="
          compact
            ? 'gap-1.5 px-1.5 min-h-7'
            : 'gap-2 sm:gap-2.5 px-2.5 sm:px-3 min-h-[44px] sm:min-h-[32px]'
        "
        @click="isMobile ? onOpenChange(true) : inputEl()?.focus()"
      >
        <div
          class="flex-shrink-0 flex items-center justify-center"
          :class="compact ? 'w-4 h-4' : 'w-7 h-7 sm:w-5 sm:h-5'"
        >
          <button
            v-if="modelValue && picker.selectedDisplayName.value && !isOpen"
            type="button"
            @click.stop="navigateToProfile"
            class="rounded-full hover:ring-2 hover:ring-accent/50 transition-all cursor-pointer"
            :title="$t('ticket-picker-user-view-profile', { name: picker.selectedDisplayName.value })"
          >
            <UserAvatar
              :uuid="modelValue"
              :fallbackName="picker.selected.value?.name"
              :fallbackAvatar="picker.selected.value?.avatar_thumb || picker.selected.value?.avatar_url || null"
              :showName="false"
              :size="compact ? 'xxs' : 'xs'"
              :clickable="false"
            />
          </button>
          <div
            v-else
            class="rounded-full bg-surface border border-subtle flex items-center justify-center transition-colors"
            :class="[
              compact ? 'w-4 h-4' : 'w-7 h-7 sm:w-5 sm:h-5',
              { 'border-accent/50 bg-accent/5': isOpen },
            ]"
          >
            <Icon name="user" size="xs" class="text-tertiary" />
          </div>
        </div>

        <div class="flex-1 min-w-0">
          <!-- Desktop: the combobox input. Mobile: a button that
               opens the sheet, where the input lives. -->
          <ComboboxInput
            v-if="!isMobile"
            ref="inputRef"
            v-model="inputText"
            :display-value="() => picker.selectedDisplayName.value"
            autocorrect="off"
            autocapitalize="off"
            spellcheck="false"
            :placeholder="inputPlaceholder"
            :aria-label="listLabel"
            class="w-full bg-transparent text-secondary placeholder-tertiary focus:outline-none leading-tight"
            :class="compact ? 'text-2xs py-0' : 'text-sm py-1'"
          />
          <button
            v-else
            type="button"
            aria-haspopup="dialog"
            :aria-expanded="isOpen"
            class="w-full text-left bg-transparent leading-tight truncate"
            :class="[
              compact ? 'text-2xs py-0' : 'text-sm py-1',
              picker.selectedDisplayName.value ? 'text-secondary' : 'text-tertiary',
            ]"
            @click.stop="onOpenChange(true)"
          >
            {{ picker.selectedDisplayName.value || inputPlaceholder }}
          </button>
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
    </ComboboxAnchor>

    <!-- Desktop popup, portalled so the SectionCard's overflow-hidden
         chrome cannot clip it. -->
    <ComboboxPortal v-if="!isMobile">
      <ComboboxContent
        position="popper"
        side="bottom"
        align="start"
        :side-offset="4"
        :collision-padding="8"
        class="popover-inner z-overlay rounded-lg border border-default bg-surface shadow-lg shadow-black/10 dark:shadow-black/30 overflow-hidden"
        :style="{ width: 'max(var(--reka-combobox-trigger-width), 280px)', maxHeight: 'min(360px, var(--reka-combobox-content-available-height))' }"
        :aria-label="listLabel"
      >
        <ComboboxViewport class="max-h-[inherit] overflow-y-auto py-1">
          <ComboboxGroup v-for="section in sections" :key="section.key" :aria-labelledby="labelId(section.key)">
            <ComboboxLabel :id="labelId(section.key)" class="px-3 pt-2 pb-1 text-3xs font-semibold uppercase tracking-wider text-tertiary select-none">
              {{ sectionLabel(section.key) }}
            </ComboboxLabel>
            <ComboboxItem
              v-for="user in section.rows"
              :key="user.uuid"
              :value="user.uuid"
              :text-value="user.name"
              class="mx-1 px-2 py-1.5 rounded-md flex items-center gap-2.5 cursor-pointer transition-colors outline-none data-[highlighted]:bg-accent/10"
            >
              <UserAvatar
                :uuid="user.uuid"
                :fallbackName="user.name"
                :fallbackAvatar="user.avatar_thumb || user.avatar_url || null"
                :showName="false"
                size="xs"
                :clickable="false"
              />
              <div class="flex-1 min-w-0">
                <div class="text-xs-plus text-primary truncate">
                  {{ user.name
                  }}<span v-if="section.key === 'you'" class="text-tertiary font-normal"> {{ $t('ticket-picker-user-you-suffix') }}</span>
                </div>
                <div v-if="user.email" class="text-2xs text-tertiary truncate">
                  {{ user.email }}
                </div>
              </div>
              <Icon v-if="user.uuid === modelValue" name="check" size="xs" class="text-accent flex-shrink-0" />
            </ComboboxItem>
          </ComboboxGroup>

          <div v-if="!hasRows && !picker.isLoading.value" class="px-3 py-6 text-center text-[12px] text-tertiary">
            {{ emptyHint() }}
          </div>
          <div v-if="picker.isLoading.value && !hasRows" class="px-3 py-6 flex items-center justify-center">
            <Spinner size="sm" />
          </div>
        </ComboboxViewport>
      </ComboboxContent>
    </ComboboxPortal>

    <!-- Mobile bottom sheet: the same combobox, its input and list
         inside the sheet. -->
    <ResponsivePanel
      v-if="isMobile"
      :open="isOpen"
      :title="type === 'assignee' ? $t('ticket-picker-user-sheet-title-assignee') : $t('ticket-picker-user-sheet-title-requester')"
      side-panel-class="w-80"
      @close="onOpenChange(false)"
    >
      <div class="px-3 pt-2 pb-1 border-b border-default">
        <ComboboxInput
          v-if="sheetListMounted"
          ref="inputRef"
          v-model="inputText"
          :display-value="() => ''"
          auto-focus
          autocorrect="off"
          autocapitalize="off"
          spellcheck="false"
          :placeholder="type === 'assignee' ? $t('ticket-picker-user-search-staff') : $t('ticket-picker-user-search-users')"
          :aria-label="listLabel"
          class="w-full px-3 py-2 rounded-md border border-default bg-surface-alt text-sm text-primary placeholder-tertiary focus:border-accent focus:ring-1 focus:ring-accent/30 focus:outline-none"
        />
      </div>
      <ComboboxContent
        position="inline"
        class="flex-1 min-h-0 flex flex-col"
        :aria-label="listLabel"
        @focus-outside="keepOpen"
        @pointer-down-outside="keepOpen"
        @vue:mounted="sheetListMounted = true"
        @vue:unmounted="sheetListMounted = false"
      >
        <ComboboxViewport class="flex-1 overflow-y-auto py-1">
          <ComboboxGroup v-for="section in sections" :key="section.key" :aria-labelledby="labelId(section.key)">
            <ComboboxLabel :id="labelId(section.key)" class="px-4 pt-3 pb-1 text-3xs font-semibold uppercase tracking-wider text-tertiary select-none">
              {{ sectionLabel(section.key) }}
            </ComboboxLabel>
            <ComboboxItem
              v-for="user in section.rows"
              :key="user.uuid"
              :value="user.uuid"
              :text-value="user.name"
              class="px-3 py-2.5 flex items-center gap-3 cursor-pointer active:bg-surface-alt transition-colors outline-none data-[highlighted]:bg-surface-hover/60"
            >
              <UserAvatar
                :uuid="user.uuid"
                :fallbackName="user.name"
                :fallbackAvatar="user.avatar_thumb || user.avatar_url || null"
                :showName="false"
                size="sm"
                :clickable="false"
              />
              <div class="flex-1 min-w-0">
                <div class="text-sm text-primary truncate">
                  {{ user.name
                  }}<span v-if="section.key === 'you'" class="text-tertiary font-normal"> {{ $t('ticket-picker-user-you-suffix') }}</span>
                </div>
                <div v-if="user.email" class="text-xs text-tertiary truncate">
                  {{ user.email }}
                </div>
              </div>
              <Icon v-if="user.uuid === modelValue" name="check" size="sm" class="text-accent flex-shrink-0" />
            </ComboboxItem>
          </ComboboxGroup>
          <div v-if="!hasRows && !picker.isLoading.value" class="px-4 py-8 text-center text-sm text-tertiary">
            {{ emptyHint() }}
          </div>
          <div v-if="picker.isLoading.value && !hasRows" class="px-3 py-8 flex items-center justify-center">
            <Spinner size="md" />
          </div>
        </ComboboxViewport>
      </ComboboxContent>
    </ResponsivePanel>
  </ComboboxRoot>
</template>
