<script setup lang="ts">
/**
 * Single "+ Add filter" entry point. Two-stage popover (Linear's
 * `F` keyboard menu pattern):
 *   Stage 1 — facet picker. List of filter types the user can
 *             add. Facets that already have an active filter are
 *             marked so the user knows which pill they'd be
 *             editing if they pick again.
 *   Stage 2 — value picker for the chosen facet. Multi-select
 *             checkbox list, or a text input for the title facet.
 *
 * Stage transitions slide horizontally — forward (right-to-left)
 * when drilling into a facet, back (left-to-right) when
 * returning. The directional cue makes the navigation feel
 * spatial: the user's mental model is "I went deeper into a
 * sub-menu and now I'm coming back out."
 *
 * Keyboard:
 *   ↑/↓        navigate facet list (stage 1) or option list (stage 2)
 *   Enter      activate
 *   Backspace  in stage 2, return to stage 1 (when not typing in
 *              the title input)
 *   ←          same as Backspace
 *   Escape     close the menu (handled by Popover)
 *
 * Exposes an imperative `openWithFacet(facet)` so the slash
 * keybinding can jump straight to stage 2 for the title facet.
 */
import { computed, nextTick, ref, watch } from 'vue'
import Icon from '@/components/common/Icon.vue'
import ResponsiveMenu from '@/components/common/ResponsiveMenu.vue'
import FilterValueList from '@/components/views/FilterValueList.vue'
import { useMenuKeyboardNav, type KeyboardNavItem } from '@/composables/useMenuKeyboardNav'
import type { PopoverAnchor } from '@/composables/usePopover'
import type { FilterOption, FacetKind } from '@/composables/useListFilters'

/** Minimal descriptor the menu needs to render and route events.
 *  Consumers map their FacetDef array to this shape; the menu
 *  stays dataset-agnostic so tickets, assets, and users all
 *  share one popover. */
export interface AddFilterFacet {
  key: string
  label: string
  kind: FacetKind
}

const props = defineProps<{
  facets: AddFilterFacet[]
  activeFacets: string[]
  optionsFor: (key: string) => FilterOption[]
  selectedFor: (key: string) => Set<string>
  textValueFor: (key: string) => string
  /** Placeholder for text-facet inputs. Defaults to the generic
   *  search-title string for backwards compat with tickets. */
  textPlaceholder?: string
}>()

const emit = defineEmits<{
  (e: 'toggle', key: string, value: string): void
  (e: 'clear', key: string): void
  (e: 'set-text', key: string, value: string): void
}>()

const triggerRef = ref<HTMLElement | null>(null)
const open = ref(false)
const stage = ref<'facets' | 'values'>('facets')
const direction = ref<'forward' | 'back'>('forward')
const activeFacet = ref<string | null>(null)
const textInputRef = ref<HTMLInputElement | null>(null)
const facetListRef = ref<HTMLDivElement | null>(null)

const anchor = computed<PopoverAnchor>(() => ({
  type: 'element',
  element: () => triggerRef.value,
}))

const activeSet = computed<Set<string>>(() => new Set(props.activeFacets))

// ---------------------------------------------------------------
// Stage 1 keyboard nav. The facet list uses the same composable
// as FilterValueList for consistency — arrow keys, type-ahead,
// Enter to drill in.
// ---------------------------------------------------------------
interface FacetNavItem extends KeyboardNavItem {
  key: string
  kind: FacetKind
}

const facetItems = computed<FacetNavItem[]>(() =>
  props.facets.map((f) => ({ label: f.label, key: f.key, kind: f.kind })),
)

const facetNav = useMenuKeyboardNav<FacetNavItem>((item) => pickFacet(item.key))

watch(facetItems, (next) => facetNav.setItems(next), { immediate: true })

function onFacetListKeydown(e: KeyboardEvent): void {
  facetNav.onKeydown(e)
}

function facetByKey(key: string): AddFilterFacet | undefined {
  return props.facets.find((f) => f.key === key)
}

function pickFacet(key: string): void {
  direction.value = 'forward'
  activeFacet.value = key
  stage.value = 'values'
  if (facetByKey(key)?.kind === 'text') {
    void nextTick(() => textInputRef.value?.focus())
  }
}

function back(): void {
  direction.value = 'back'
  stage.value = 'facets'
  activeFacet.value = null
  void nextTick(() => facetListRef.value?.focus())
}

function reset(): void {
  stage.value = 'facets'
  direction.value = 'forward'
  activeFacet.value = null
  facetNav.reset()
}

function onClose(): void {
  open.value = false
  reset()
}

function onTextInput(e: Event): void {
  if (!activeFacet.value) return
  const t = e.target as HTMLInputElement
  emit('set-text', activeFacet.value, t.value)
}

function onTextKeydown(e: KeyboardEvent): void {
  // Allow backspace to remove characters; only treat empty-input
  // backspace as "go back to stage 1". The user can hit Escape
  // (handled by Popover) to close from the text input.
  if (e.key === 'Backspace') {
    const value = (e.target as HTMLInputElement).value
    if (value === '') {
      e.preventDefault()
      back()
    }
  }
}

function onValueStageKeydown(e: KeyboardEvent): void {
  // Backspace and Left arrow both go back, matching Notion /
  // Raycast multi-step menu vocabulary. Don't intercept when
  // focus is on the title text input (handled separately above).
  if (e.target === textInputRef.value) return
  if (e.key === 'Backspace' || e.key === 'ArrowLeft') {
    e.preventDefault()
    back()
  }
}

function openWithFacet(key: string): void {
  open.value = true
  pickFacet(key)
}

defineExpose({ openWithFacet })

watch(open, (next) => {
  if (next && stage.value === 'facets') {
    void nextTick(() => facetListRef.value?.focus())
  }
})

const stageOptions = computed<FilterOption[]>(() =>
  activeFacet.value ? props.optionsFor(activeFacet.value) : [],
)
const stageSelected = computed<Set<string>>(() =>
  activeFacet.value ? props.selectedFor(activeFacet.value) : new Set(),
)
const stageTextValue = computed<string>(() =>
  activeFacet.value ? props.textValueFor(activeFacet.value) : '',
)
const stageMeta = computed<{ label: string; kind: FacetKind } | null>(() => {
  if (!activeFacet.value) return null
  const f = facetByKey(activeFacet.value)
  return f ? { label: f.label, kind: f.kind } : null
})
</script>

<template>
  <div class="inline-flex">
    <button
      ref="triggerRef"
      type="button"
      class="inline-flex items-center gap-1 text-2xs px-2 h-6 rounded-md border border-dashed transition-colors"
      :class="open
        ? 'border-default bg-surface-hover text-primary'
        : 'text-tertiary hover:text-primary border-subtle hover:border-default hover:bg-surface-hover'"
      :aria-expanded="open"
      aria-haspopup="menu"
      @click="open = !open"
    >
      <Icon name="add" class="w-3 h-3" />
      <span>{{ $t('views-add-filter-trigger') }}</span>
    </button>

    <ResponsiveMenu
      :open="open"
      :anchor="anchor"
      :title="stage === 'values' && stageMeta ? stageMeta.label : $t('views-add-filter-trigger')"
      placement="bottom-start"
      react-to-scroll="reposition"
      :offset="4"
      role="menu"
      :auto-focus="false"
      popover-class="bg-surface border border-default rounded-lg shadow-xl overflow-hidden min-w-[15rem] max-w-[calc(100vw-1rem)] sm:max-w-[22rem]"
      @close="onClose"
    >
      <!-- Stage container — the relative + overflow-hidden wrap
           lets the slide-in / slide-out happen inside a fixed
           viewport so neighbouring chrome doesn't shift. The
           data-attribute drives the transition direction so back
           navigation reverses the slide. -->
      <div
        class="relative overflow-hidden"
        :data-stage-direction="direction"
      >
        <Transition :name="`stage-slide-${direction}`" mode="out-in">
          <!-- Stage 1: facet picker -->
          <div
            v-if="stage === 'facets'"
            ref="facetListRef"
            tabindex="-1"
            role="menu"
            class="py-1 outline-none"
            @keydown="onFacetListKeydown"
          >
            <button
              v-for="(facet, i) in facets"
              :key="facet.key"
              type="button"
              role="menuitem"
              class="w-full px-3 py-1.5 flex items-center gap-2 text-left transition-colors duration-75"
              :class="facetNav.highlightedIndex.value === i
                ? 'bg-accent/10'
                : 'hover:bg-surface-hover'"
              @click.stop="pickFacet(facet.key)"
              @mouseenter="facetNav.setHighlighted(i)"
            >
              <span class="flex-1 text-xs text-primary">{{ facet.label }}</span>
              <Icon
                v-if="activeSet.has(facet.key)"
                name="check"
                class="w-3 h-3 text-accent"
              />
              <Icon name="chevronRight" class="w-3 h-3 text-tertiary" />
            </button>
          </div>

          <!-- Stage 2: value picker -->
          <div v-else @keydown="onValueStageKeydown">
            <header
              class="flex items-center gap-2 px-3 py-1.5 border-b border-subtle"
            >
              <button
                type="button"
                class="text-tertiary hover:text-primary transition-colors p-0.5 -ml-0.5 rounded hover:bg-surface-hover"
                :title="$t('views-add-filter-back-tooltip')"
                @click="back"
              >
                <Icon name="chevronLeft" class="w-3.5 h-3.5" />
              </button>
              <span class="text-xs font-medium text-primary">{{ stageMeta?.label }}</span>
            </header>

            <div v-if="stageMeta && stageMeta.kind === 'text'" class="p-2">
              <input
                ref="textInputRef"
                type="text"
                :value="stageTextValue"
                :placeholder="textPlaceholder ?? $t('views-add-filter-search-title-placeholder')"
                class="bg-surface border border-subtle rounded-md text-xs px-2 h-7 w-full text-primary placeholder:text-tertiary focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
                @input="onTextInput"
                @keydown="onTextKeydown"
              />
            </div>

            <FilterValueList
              v-else
              :options="stageOptions"
              :selected="stageSelected"
              :empty-message="$t('views-add-filter-no-matches')"
              @toggle="(v) => activeFacet && emit('toggle', activeFacet, v)"
              @clear="activeFacet && emit('clear', activeFacet)"
            />
          </div>
        </Transition>
      </div>
    </ResponsiveMenu>
  </div>
</template>

<style>
/* Forward direction (drilling into a facet): the new stage
   slides in from the right while the outgoing one slides out
   to the left. Same curve as the popover open transition for
   visual coherence. */
.stage-slide-forward-enter-active,
.stage-slide-forward-leave-active {
  transition:
    opacity 160ms cubic-bezier(0.16, 1, 0.3, 1),
    transform 160ms cubic-bezier(0.16, 1, 0.3, 1);
}
.stage-slide-forward-enter-from {
  opacity: 0;
  transform: translateX(12px);
}
.stage-slide-forward-leave-to {
  opacity: 0;
  transform: translateX(-12px);
}

/* Back direction (returning to facet picker): mirror image. */
.stage-slide-back-enter-active,
.stage-slide-back-leave-active {
  transition:
    opacity 160ms cubic-bezier(0.16, 1, 0.3, 1),
    transform 160ms cubic-bezier(0.16, 1, 0.3, 1);
}
.stage-slide-back-enter-from {
  opacity: 0;
  transform: translateX(-12px);
}
.stage-slide-back-leave-to {
  opacity: 0;
  transform: translateX(12px);
}

@media (prefers-reduced-motion: reduce) {
  .stage-slide-forward-enter-active,
  .stage-slide-forward-leave-active,
  .stage-slide-back-enter-active,
  .stage-slide-back-leave-active {
    transition: opacity 100ms linear;
  }
  .stage-slide-forward-enter-from,
  .stage-slide-forward-leave-to,
  .stage-slide-back-enter-from,
  .stage-slide-back-leave-to {
    transform: none;
  }
}
</style>
