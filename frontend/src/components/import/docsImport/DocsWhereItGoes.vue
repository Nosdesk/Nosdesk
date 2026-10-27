<script setup lang="ts">
/**
 * Pages' "Where it goes": the collection the pages land in, their status,
 * what happens to images linked from the web, and patterns that leave files
 * out. Every choice shows in the tree above at once.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import SegmentedControl from '@/components/common/SegmentedControl.vue'
import type {
  DocsCollectionMode,
  DocsProfile,
  DocsStatusMode,
  DocsWebImages,
} from '@nosdesk/core/services/docsImportService'

import type { PagesChoices } from './docsTreeModel'

export interface CollectionChoice {
  id: number
  name: string
  pages: number
}

const props = defineProps<{
  profile: DocsProfile
  /** The new collection's default name: the server's. */
  defaultName: string
  /** The collections a folder per collection would make. */
  folderCollections: string[]
  sourceCollections: number
  collections: CollectionChoice[]
  /** Pages of the chosen existing collection, for "Under page". */
  underPages: { id: number; title: string; depth: number }[]
  /** An existing collection named like the import, preselected. */
  suggested: CollectionChoice | null
  webImages: number
  /** Pages the admin's patterns leave out. */
  leftOut: number
  disabled?: boolean
}>()

const choices = defineModel<PagesChoices>({ required: true })

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

function set<K extends keyof PagesChoices>(key: K, value: PagesChoices[K]) {
  choices.value = { ...choices.value, [key]: value }
}

function setMode(mode: DocsCollectionMode) {
  const next: PagesChoices = { ...choices.value, mode }
  if (mode === 'existing' && next.collectionId == null) next.collectionId = props.collections[0]?.id ?? null
  if (mode !== 'existing') next.underPageId = null
  choices.value = next
}

function setCollection(value: string) {
  choices.value = { ...choices.value, collectionId: value ? Number(value) : null, underPageId: null }
}

const nameClash = computed(() => {
  if (choices.value.mode !== 'new') return null
  const name = (choices.value.collectionName.trim() || props.defaultName).toLowerCase()
  return props.collections.find((c) => c.name.toLowerCase() === name) ?? null
})

const modes = computed(() => {
  const out: { value: DocsCollectionMode; label: string; desc: string }[] = [
    {
      value: 'existing',
      label: t('docs-import-where-existing'),
      desc: props.collections.length > 0 ? t('docs-import-where-existing-desc') : t('docs-import-where-existing-none'),
    },
    {
      value: 'new',
      label: t('docs-import-where-new'),
      desc: nameClash.value ? t('docs-import-where-new-clash', { name: nameClash.value.name }) : '',
    },
    {
      value: 'per_top_level_folder',
      label: t('docs-import-where-per-folder'),
      desc: t('docs-import-where-per-folder-desc', {
        count: props.folderCollections.length,
        names: props.folderCollections.slice(0, 6).join(', '),
      }),
    },
  ]
  if (props.profile === 'nosdesk_v2') {
    out.push({
      value: 'as_source',
      label: t('docs-import-where-as-source'),
      desc: t('docs-import-where-as-source-desc', { count: props.sourceCollections }),
    })
  }
  return out
})

const statusOptions = computed(() => [
  { value: 'drafts' as DocsStatusMode, label: t('docs-import-status-drafts') },
  { value: 'published' as DocsStatusMode, label: t('docs-import-status-published') },
  { value: 'as_files' as DocsStatusMode, label: t('docs-import-status-as-files') },
])

const statusHelp = computed(() => {
  switch (choices.value.status) {
    case 'published':
      return t('docs-import-status-published-help')
    case 'as_files':
      return props.profile === 'notion'
        ? t('docs-import-status-as-files-notion')
        : t('docs-import-status-as-files-help')
    default:
      return t('docs-import-status-drafts-help')
  }
})

const webOptions = computed(() => [
  { value: 'download' as DocsWebImages, label: t('docs-import-web-download') },
  { value: 'links' as DocsWebImages, label: t('docs-import-web-links') },
])

const webHelp = computed(() =>
  choices.value.webImages === 'download'
    ? t('docs-import-web-download-help', { count: props.webImages })
    : t('docs-import-web-links-help', { count: props.webImages }),
)

const draft = ref('')

function addPattern() {
  const g = draft.value.trim()
  if (!g) return
  if (!choices.value.typed.includes(g)) set('typed', [...choices.value.typed, g])
  draft.value = ''
}

function removePattern(i: number) {
  set(
    'typed',
    choices.value.typed.filter((_, j) => j !== i),
  )
}

const inputClass =
  'h-8 px-2.5 rounded-lg border border-default bg-surface text-primary text-xs-plus min-w-0 hover:border-strong focus:outline-2 focus:outline-accent focus:border-accent'
</script>

<template>
  <div class="flex flex-col gap-5" data-test="docs-where">
    <fieldset class="flex flex-col gap-2 min-w-0" :disabled="disabled">
      <legend class="text-xs font-medium text-tertiary uppercase tracking-wide mb-1.5">
        {{ $t('docs-import-where-collection') }}
      </legend>
      <p v-if="suggested" class="text-xs text-tertiary" data-test="docs-where-suggested">
        {{ $t('docs-import-where-suggested', { name: suggested.name, count: suggested.pages }) }}
      </p>
      <div class="flex flex-col gap-2">
        <label
          v-for="m in modes"
          :key="m.value"
          class="flex items-start gap-2.5 px-3 py-2.5 border rounded-lg cursor-pointer bg-surface min-w-0"
          :class="choices.mode === m.value ? 'border-accent ring-1 ring-accent' : 'border-default hover:border-strong'"
          :data-test="`docs-where-mode-${m.value}`"
        >
          <input
            type="radio"
            name="docs-import-collections"
            class="mt-0.5 w-4 h-4 shrink-0 accent-accent"
            :value="m.value"
            :checked="choices.mode === m.value"
            :disabled="m.value === 'existing' && collections.length === 0"
            @change="setMode(m.value)"
          />
          <span class="flex-1 min-w-0 flex flex-col gap-1.5">
            <span class="text-sm font-medium text-primary">{{ m.label }}</span>
            <span v-if="m.desc" class="text-xs text-tertiary">{{ m.desc }}</span>
            <span v-if="choices.mode === m.value && m.value === 'existing'" class="flex flex-wrap gap-2">
              <select
                :class="inputClass"
                class="flex-[1_1_180px]"
                :value="choices.collectionId ?? ''"
                :aria-label="$t('docs-import-where-collection')"
                data-test="docs-where-collection"
                @change="setCollection(($event.target as HTMLSelectElement).value)"
              >
                <option v-for="c in collections" :key="c.id" :value="c.id">
                  {{ $t('docs-import-where-collection-option', { name: c.name, count: c.pages }) }}
                </option>
              </select>
              <select
                :class="inputClass"
                class="flex-[1_1_180px]"
                :value="choices.underPageId ?? ''"
                :aria-label="$t('docs-import-where-under')"
                data-test="docs-where-under"
                @change="set('underPageId', ($event.target as HTMLSelectElement).value ? Number(($event.target as HTMLSelectElement).value) : null)"
              >
                <option value="">{{ $t('docs-import-where-top-level') }}</option>
                <option v-for="p in underPages" :key="p.id" :value="p.id">
                  {{ '  '.repeat(p.depth) }}{{ $t('docs-import-where-under-option', { title: p.title }) }}
                </option>
              </select>
            </span>
            <span v-if="choices.mode === m.value && m.value === 'new'" class="flex">
              <input
                type="text"
                :class="inputClass"
                class="flex-[1_1_180px]"
                :value="choices.collectionName"
                :placeholder="defaultName"
                :aria-label="$t('docs-import-where-name')"
                maxlength="255"
                data-test="docs-where-name"
                @input="set('collectionName', ($event.target as HTMLInputElement).value)"
              />
            </span>
          </span>
        </label>
      </div>
    </fieldset>

    <div class="flex flex-wrap items-start gap-x-4 gap-y-2">
      <div class="flex-[1_1_220px] min-w-0 flex flex-col gap-0.5">
        <span class="text-sm font-medium text-primary">{{ $t('docs-import-status-label') }}</span>
        <span class="text-xs text-tertiary" data-test="docs-where-status-help">{{ statusHelp }}</span>
      </div>
      <div class="max-w-full overflow-x-auto scrollbar-hide">
        <SegmentedControl
          :model-value="choices.status"
          :options="statusOptions"
          :aria-label="$t('docs-import-status-label')"
          @update:model-value="set('status', $event)"
        />
      </div>
    </div>

    <div class="flex flex-wrap items-start gap-x-4 gap-y-2">
      <div class="flex-[1_1_220px] min-w-0 flex flex-col gap-0.5">
        <span class="text-sm font-medium text-primary">{{ $t('docs-import-web-label') }}</span>
        <span class="text-xs text-tertiary">{{ webHelp }}</span>
      </div>
      <div class="max-w-full overflow-x-auto scrollbar-hide">
        <SegmentedControl
          :model-value="choices.webImages"
          :options="webOptions"
          :aria-label="$t('docs-import-web-label')"
          @update:model-value="set('webImages', $event)"
        />
      </div>
    </div>

    <div class="flex flex-col gap-1.5 min-w-0">
      <label class="text-sm font-medium text-primary" for="docs-import-glob">{{ $t('docs-import-leave-out-label') }}</label>
      <div class="flex flex-wrap items-center gap-1.5">
        <span
          v-for="(g, i) in choices.typed"
          :key="g"
          class="inline-flex items-center gap-1.5 h-[26px] pl-2.5 pr-1.5 rounded-full bg-surface-alt border border-default text-xs max-w-full"
          data-test="docs-leave-out-chip"
        >
          <span class="font-mono truncate">{{ g }}</span>
          <button
            type="button"
            class="w-[18px] h-[18px] rounded-full flex items-center justify-center text-tertiary hover:bg-surface-hover hover:text-primary"
            :aria-label="$t('docs-import-leave-out-remove', { pattern: g })"
            @click="removePattern(i)"
          >
            <Icon name="close" size="xs" />
          </button>
        </span>
        <input
          id="docs-import-glob"
          v-model="draft"
          type="text"
          :class="inputClass"
          class="flex-[1_1_160px] font-mono"
          :placeholder="$t('docs-import-leave-out-placeholder')"
          aria-describedby="docs-import-glob-help"
          autocomplete="off"
          spellcheck="false"
          :disabled="disabled"
          data-test="docs-leave-out-input"
          @keydown.enter.prevent="addPattern"
        />
        <Button variant="secondary" size="sm" :disabled="!draft.trim() || disabled" data-test="docs-leave-out-add" @click="addPattern">
          {{ $t('docs-import-leave-out-add') }}
        </Button>
      </div>
      <p id="docs-import-glob-help" class="text-xs text-tertiary">
        {{ $t('docs-import-leave-out-help') }}
        <template v-if="choices.typed.length > 0">{{ $t('docs-import-leave-out-matches', { count: leftOut }) }}</template>
      </p>
    </div>
  </div>
</template>
