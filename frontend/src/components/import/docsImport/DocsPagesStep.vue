<script setup lang="ts">
/**
 * Pages: the plan, shaped before anything is checked. The line on top says
 * what the archive looks like (and reads it as another tool's export on
 * request); the tree shows the pages as Nosdesk will, with a tick each;
 * "Where it goes" picks the collection, status, web images and patterns to
 * leave out. Check counts what is ticked and runs the dry run.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import Spinner from '@/components/common/Spinner.vue'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'
import {
  DOCS_PROFILES,
  type DocsProfile,
  type DocsSource,
} from '@nosdesk/core/services/docsImportService'

import DocsPageTree from './DocsPageTree.vue'
import DocsWhereItGoes, { type CollectionChoice } from './DocsWhereItGoes.vue'
import {
  toggle,
  treeCounts,
  type CollectionGroup,
  type PagesChoices,
  type Tree,
  type TreeNode,
} from './docsTreeModel'

const props = defineProps<{
  filename: string
  source: DocsSource
  tree: Tree
  groups: CollectionGroup[]
  collections: CollectionChoice[]
  underPages: { id: number; title: string; depth: number }[]
  suggested: CollectionChoice | null
  checking?: boolean
  rereading?: boolean
  discarding?: boolean
}>()

const choices = defineModel<PagesChoices>('choices', { required: true })
const off = defineModel<Set<string>>('off', { required: true })
const expanded = defineModel<Set<string>>('expanded', { required: true })

const emit = defineEmits<{
  check: []
  'change-file': []
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)
const fmt = (n: number) => n.toLocaleString()

const view = computed(() => props.source.tree)
const counts = computed(() => treeCounts(props.tree, off.value, choices.value.typed))
const busy = computed(() => !!props.checking || !!props.rereading || !!props.discarding)

/** The profile the tree was read as, and whether detection chose it. */
const detectedProfile = computed<DocsProfile>(() => view.value.detected ?? view.value.profile)
const sourceLine = computed(() => {
  const profile = t(`docs-import-profile-${view.value.profile.replace(/_/g, '-')}`)
  const pages = view.value.pages.length
  const images = view.value.images + view.value.web_images
  return view.value.detected
    ? t('docs-import-source-read-as', { profile, pages, images })
    : t('docs-import-source-looks-like', { profile, pages, images })
})

const profileOptions = computed(() =>
  DOCS_PROFILES.map((p) => ({
    value: p,
    label:
      p === detectedProfile.value
        ? t('docs-import-profile-detected', { profile: t(`docs-import-profile-option-${p.replace(/_/g, '-')}`) })
        : t(`docs-import-profile-option-${p.replace(/_/g, '-')}`),
  })),
)

function chooseProfile(value: string) {
  const profile = value as DocsProfile
  // Detection's own answer goes back to detection.
  choices.value = { ...choices.value, profile: profile === detectedProfile.value ? null : profile }
}

const allExpanded = computed(() =>
  props.tree.all.filter((n) => n.children.length > 0).every((n) => expanded.value.has(n.page.path)),
)

function expandAll() {
  expanded.value = new Set(props.tree.all.filter((n) => n.children.length > 0).map((n) => n.page.path))
}

function collapseAll() {
  expanded.value = new Set()
}

function onToggle(node: TreeNode) {
  off.value = toggle(node, off.value)
}

const folderCollections = computed(() => props.tree.roots.map((n) => n.page.title))

const showChangeFile = ref(false)
</script>

<template>
  <section class="flex flex-col gap-6" data-test="docs-pages">
    <div class="flex flex-col sm:flex-row sm:flex-wrap sm:items-center gap-1 sm:gap-2.5 text-sm min-w-0" data-test="docs-source-line">
      <span class="inline-flex items-center gap-2 min-w-0">
        <Icon name="archive" size="sm" class="text-tertiary shrink-0" />
        <span class="font-medium text-primary truncate">{{ filename }}</span>
        <span class="text-tertiary shrink-0">· {{ formatFileSize(source.size) }}</span>
      </span>
      <span class="hidden sm:inline text-tertiary">·</span>
      <span class="inline-flex flex-wrap items-center gap-2 min-w-0" aria-live="polite">
        <template v-if="rereading">
          <Spinner size="sm" />
          <span class="text-secondary">{{ $t('docs-import-source-rereading') }}</span>
        </template>
        <span v-else class="text-secondary" data-test="docs-source-detected">{{ sourceLine }}</span>
        <span class="relative inline-flex items-center gap-1 text-accent">
          <span>{{ $t('docs-import-source-change') }}</span>
          <Icon name="chevronDown" size="xs" />
          <select
            class="absolute inset-0 opacity-0 w-full cursor-pointer"
            :value="view.profile"
            :aria-label="$t('docs-import-source-change-label')"
            :disabled="busy"
            data-test="docs-source-profile"
            @change="chooseProfile(($event.target as HTMLSelectElement).value)"
          >
            <option v-for="p in profileOptions" :key="p.value" :value="p.value">{{ p.label }}</option>
          </select>
        </span>
      </span>
    </div>

    <SectionCard content-padding="" data-test="docs-pages-tree">
      <template #title>{{ $t('docs-import-tree-title') }}</template>
      <template #headerActions>
        <button
          type="button"
          class="text-2xs font-medium text-accent hover:underline"
          data-test="docs-tree-expand-all"
          @click="allExpanded ? collapseAll() : expandAll()"
        >
          {{ allExpanded ? $t('docs-import-tree-collapse-all') : $t('docs-import-tree-expand-all') }}
        </button>
      </template>
      <DocsPageTree
        v-model:expanded="expanded"
        :tree="tree"
        :groups="groups"
        :off="off"
        :patterns="choices.typed"
        :label="$t('docs-import-tree-title')"
        @toggle="onToggle"
      />
      <div class="flex flex-wrap items-center gap-2 px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">
        <span aria-live="polite" data-test="docs-tree-count">
          <b class="text-primary font-semibold">{{ $t('docs-import-tree-count', { ticked: fmt(counts.ticked), total: fmt(counts.total) }) }}</b>
          <template v-if="counts.unticked > 0"> · {{ $t('docs-import-tree-count-unticked', { count: counts.unticked }) }}</template>
          <template v-if="counts.leftOut > 0"> · {{ $t('docs-import-tree-count-left-out', { count: counts.leftOut }) }}</template>
        </span>
        <span class="hidden sm:inline ml-auto">{{ $t('docs-import-tree-follow') }}</span>
      </div>
    </SectionCard>

    <SectionCard content-padding="p-4">
      <template #title>{{ $t('docs-import-where-title') }}</template>
      <DocsWhereItGoes
        v-model="choices"
        :profile="view.profile"
        :default-name="view.collections[0] ?? filename"
        :folder-collections="folderCollections"
        :source-collections="view.collections.length"
        :collections="collections"
        :under-pages="underPages"
        :suggested="suggested"
        :web-images="view.web_images"
        :left-out="counts.leftOut"
        :disabled="busy"
      />
    </SectionCard>

    <div class="sticky bottom-0 z-10 bg-app border-t border-default py-3 flex flex-wrap items-center gap-3" data-test="docs-pages-bar">
      <div v-if="checking" class="w-full flex items-start gap-2 text-xs-plus text-secondary" role="status" data-test="docs-checking">
        <Spinner size="sm" class="mt-0.5" />
        <span>
          {{
            choices.webImages === 'download' && view.web_images > 0
              ? $t('docs-import-checking-line-web', { count: counts.ticked, images: view.images, web: view.web_images })
              : $t('docs-import-checking-line', { count: counts.ticked, images: view.images })
          }}
        </span>
      </div>
      <Button
        variant="ghost"
        class="order-2 sm:order-none w-full sm:w-auto"
        :loading="discarding"
        :disabled="busy"
        data-test="docs-change-file"
        @click="showChangeFile = true"
      >
        {{ $t('docs-import-change-file') }}
      </Button>
      <span v-if="counts.ticked === 0" class="order-1 sm:order-none w-full sm:w-auto sm:ml-auto text-xs text-tertiary">
        {{ $t('docs-import-tick-one') }}
      </span>
      <Button
        class="order-first sm:order-none w-full sm:w-auto"
        :class="counts.ticked === 0 ? '' : 'sm:ml-auto'"
        :disabled="counts.ticked === 0 || busy"
        :loading="checking"
        data-test="docs-check"
        @click="emit('check')"
      >
        {{ checking ? $t('docs-import-checking') : $t('docs-import-check', { count: counts.ticked }) }}
      </Button>
    </div>

    <ConfirmModal
      :show="showChangeFile"
      :title="$t('docs-import-change-file-title')"
      :message="$t('docs-import-change-file-message')"
      :confirm-label="$t('docs-import-change-file-confirm')"
      variant="danger"
      @close="showChangeFile = false"
      @confirm="showChangeFile = false; emit('change-file')"
    />
  </section>
</template>
