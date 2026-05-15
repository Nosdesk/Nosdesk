<script setup lang="ts">
/**
 * Per-ticket tag picker + chip row. Sidebar surface that lets
 * staff attach / detach workspace tags from a ticket.
 *
 * UX:
 *   - Render attached tags as dismissable colour chips.
 *   - "Add tag" trigger opens a popover with a typeahead
 *     filtering the workspace tag list. Keyboard-navigable.
 *   - Inline "Create + assign" when the typed text matches no
 *     existing tag (admins only — non-admins see "no matches"
 *     and have to ask an admin to create the tag).
 *   - Empty + non-focused state shows nothing but the "Add tag"
 *     trigger so the sidebar stays quiet for tickets without
 *     tags.
 *
 * Mutations go through the parent's emit (`update:tag-ids`) so
 * the page-level data layer (useTicketData) handles persistence
 * + optimistic updates uniformly with status / assignee /
 * priority writes.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'
import { useTagsStore } from '@/stores/tags'
import { useAuthStore } from '@/stores/auth'
import { tagService } from '@/services/tagService'
import { useQueryCache } from '@pinia/colada'
import { TAGS_QUERY_KEY } from '@/stores/tags'
import type { Tag } from '@/types/tag'
import Icon from '@/components/common/Icon.vue'

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const props = defineProps<{
  /** Ticket id — only needed when the user creates a brand-new
   *  tag from this picker; the create then auto-assigns to this
   *  ticket. Existing-tag selection routes through the parent's
   *  emit instead. */
  ticketId: number
  tagIds: number[]
}>()

const emit = defineEmits<{
  (e: 'update:tag-ids', value: number[]): void
}>()

const tagsStore = useTagsStore()
const authStore = useAuthStore()
const queryCache = useQueryCache()

const pickerOpen = ref(false)
const query = ref('')
const inputRef = ref<HTMLInputElement | null>(null)

// Resolved chip rows for the currently-attached ids. Tag rows
// missing from the workspace store (deleted / archived after
// attachment) get a placeholder row so the chip still renders
// rather than silently disappearing — losing a tag from the UI
// without a hint would read as a bug.
const attachedTags = computed<Tag[]>(() =>
  props.tagIds.map((id) => {
    const found = tagsStore.findById(id)
    if (found) return found
    return {
      id,
      name: `#${id}`,
      color: null,
      description: null,
      created_at: '',
      updated_at: '',
      archived_at: null,
    }
  }),
)

// Picker results: workspace tags not already attached to this
// ticket, filtered by the typed query (case-insensitive substring
// match on name). Capped at 8 for visual density; users typing
// anything specific will find their target in the first few hits.
const pickerResults = computed<Tag[]>(() => {
  const attachedSet = new Set(props.tagIds)
  const trimmed = query.value.trim().toLowerCase()
  const filtered = tagsStore.tags
    .filter((t) => !attachedSet.has(t.id))
    .filter((t) => !trimmed || t.name.toLowerCase().includes(trimmed))
  return filtered.slice(0, 8)
})

// Surface the "Create new tag" affordance only when:
//   - the user is an admin (tag-creation is admin-gated upstream),
//   - they've typed something,
//   - that text doesn't match an existing active tag exactly.
const canCreateInline = computed<boolean>(() => {
  if (!authStore.isAdmin) return false
  const trimmed = query.value.trim()
  if (trimmed.length === 0) return false
  return !tagsStore.tags.some(
    (t) => t.name.toLowerCase() === trimmed.toLowerCase(),
  )
})

function openPicker() {
  pickerOpen.value = true
  query.value = ''
  // Defer the focus so the input exists in the DOM by the time
  // we reach for it. requestAnimationFrame is good enough — the
  // popover is a regular conditional render, not a portal.
  requestAnimationFrame(() => inputRef.value?.focus())
}

function closePicker() {
  pickerOpen.value = false
  query.value = ''
}

function attachTag(id: number) {
  if (props.tagIds.includes(id)) return
  emit('update:tag-ids', [...props.tagIds, id])
  closePicker()
}

function detachTag(id: number) {
  emit('update:tag-ids', props.tagIds.filter((t) => t !== id))
}

const creating = ref(false)
async function createAndAttach() {
  const trimmed = query.value.trim()
  if (trimmed.length === 0 || creating.value) return
  creating.value = true
  try {
    const created = await tagService.create({ name: trimmed })
    // Refresh the workspace cache so the new tag shows up
    // everywhere (other open tickets, future picker opens). The
    // current attachment goes through the standard emit path so
    // the optimistic update + persistence stay uniform.
    await queryCache.invalidateQueries({ key: TAGS_QUERY_KEY })
    attachTag(created.id)
  } catch (err) {
    console.error('Failed to create tag', err)
  } finally {
    creating.value = false
  }
}

// ---- Colour mapping --------------------------------------------
//
// Tags carry a colour token from a small named palette. Mapping
// to a Tailwind chip style here means the workspace can recolour
// a tag without code changes, and unknown / null tokens degrade
// to the neutral 'gray' variant.

const CHIP_STYLES: Record<string, string> = {
  gray:   'bg-zinc-500/15 text-zinc-700 dark:text-zinc-300',
  slate:  'bg-zinc-500/15 text-zinc-700 dark:text-zinc-300',
  blue:   'bg-blue-500/15 text-blue-700 dark:text-blue-300',
  purple: 'bg-violet-500/15 text-violet-700 dark:text-violet-300',
  green:  'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300',
  amber:  'bg-amber-500/15 text-amber-700 dark:text-amber-300',
  rose:   'bg-rose-500/15 text-rose-700 dark:text-rose-300',
  subtle: 'bg-surface-alt text-secondary',
}

function chipClass(tag: Tag): string {
  return CHIP_STYLES[tag.color ?? 'gray'] ?? CHIP_STYLES.gray
}
</script>

<template>
  <div class="flex flex-col gap-1.5">
    <!-- Header is the click target for the picker. Same
         negative-margin idiom as PropertyChipRow: `-mx-2 px-2`
         extends the button 8px past the property-list
         container's content edge into the breathing area the
         container reserves via its own `px-2`, while keeping
         the label text aligned at the same x as plain <h3>
         rows. See TicketDetails for the layered padding math. -->
    <button
      type="button"
      class="group flex items-center justify-between gap-2 -mx-2 px-2 py-1 rounded text-left hover:bg-surface-hover transition-colors"
      :title="t('ticket-field-tags-add')"
      @click="openPicker"
    >
      <h3 class="text-xs font-medium text-tertiary group-hover:text-secondary transition-colors">{{ t('ticket-field-tags-label') }}</h3>
      <Icon
        name="add"
        class="w-3.5 h-3.5 text-tertiary opacity-0 group-hover:opacity-100 transition-opacity"
        aria-hidden="true"
      />
    </button>

    <!-- Attached tag chips. Each chip is dismissable via the
         trailing X button. Wraps when many tags attached so the
         row stays compact even on a narrow sidebar. The
         `-mx-2 px-2` mirrors the heading button's negative-
         margin trick so the chip row's structural footprint
         matches the button above (chips still sit at the same
         x as plain content; only the container extends). -->
    <div
      v-if="attachedTags.length > 0"
      class="flex flex-wrap items-center gap-1 -mx-2 px-2"
    >
      <span
        v-for="tag in attachedTags"
        :key="tag.id"
        class="inline-flex items-center gap-1 pl-2 pr-1 py-0.5 rounded text-[11px] font-medium"
        :class="chipClass(tag)"
        :title="tag.description || tag.name"
      >
        {{ tag.name }}
        <button
          type="button"
          class="inline-flex items-center justify-center w-4 h-4 rounded hover:bg-black/10 dark:hover:bg-white/10 transition-colors"
          :title="t('ticket-field-tags-remove', { name: tag.name })"
          @click="detachTag(tag.id)"
        >
          <Icon name="close" class="w-3 h-3" />
        </button>
      </span>
    </div>

    <!-- Picker popover. Inline rather than teleported because
         the sidebar already has a constrained width and the
         popover lives directly under its trigger; floating UI
         would be overkill for a list of <8 rows. -->
    <div
      v-if="pickerOpen"
      class="bg-surface-alt rounded-lg border border-default p-1.5 flex flex-col gap-1"
    >
      <input
        ref="inputRef"
        v-model="query"
        type="text"
        :placeholder="t('ticket-field-tags-picker-placeholder')"
        class="w-full bg-app rounded-md border border-subtle text-sm text-primary px-2 py-1 outline-none focus:border-accent"
        @keydown.escape.prevent="closePicker"
        @keydown.enter.prevent="canCreateInline ? createAndAttach() : pickerResults[0] && attachTag(pickerResults[0].id)"
      />
      <div v-if="tagsStore.isLoading" class="px-2 py-1 text-xs text-tertiary">
        {{ t('ticket-field-tags-loading') }}
      </div>
      <div
        v-else-if="pickerResults.length === 0 && !canCreateInline"
        class="px-2 py-1 text-xs text-tertiary"
      >
        {{ t('ticket-field-tags-no-match') }}
      </div>
      <button
        v-for="tag in pickerResults"
        :key="tag.id"
        type="button"
        class="flex items-center gap-2 px-2 py-1 rounded hover:bg-surface-hover text-left text-sm text-primary"
        @click="attachTag(tag.id)"
      >
        <span
          class="inline-block w-2 h-2 rounded-full"
          :class="(CHIP_STYLES[tag.color ?? 'gray'] ?? CHIP_STYLES.gray).split(' ')[0]"
          aria-hidden="true"
        />
        {{ tag.name }}
      </button>
      <button
        v-if="canCreateInline"
        type="button"
        class="flex items-center gap-2 px-2 py-1 rounded hover:bg-surface-hover text-left text-sm text-accent border-t border-default"
        :disabled="creating"
        @click="createAndAttach"
      >
        <Icon name="add" class="w-3.5 h-3.5" />
        <span>{{ creating ? t('ticket-field-tags-creating') : t('ticket-field-tags-create', { name: query.trim() }) }}</span>
      </button>
      <button
        type="button"
        class="text-[11px] text-tertiary hover:text-primary px-2 py-1 self-end"
        @click="closePicker"
      >{{ t('ticket-field-tags-done') }}</button>
    </div>
  </div>
</template>
