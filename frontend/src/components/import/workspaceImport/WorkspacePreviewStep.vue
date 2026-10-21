<script setup lang="ts">
/**
 * The Preview step, the trust surface. In order: what comes across, who the
 * people become, how they sign in, what this workspace gives up, what
 * arrives switched off, what doesn't move, and what the importer couldn't
 * resolve. One long page with a jump row. Nothing changes until Import,
 * which asks for this workspace's name.
 */
import { computed, ref } from 'vue'
import { useFluent } from 'fluent-vue'

import Button from '@/components/common/Button.vue'
import Callout from '@/components/common/Callout.vue'
import ConfirmModal from '@/components/common/ConfirmModal.vue'
import FormInput from '@/components/common/FormInput.vue'
import Icon from '@/components/common/Icon.vue'
import SectionCard from '@/components/common/SectionCard.vue'
import { formatFileSize } from '@nosdesk/core/utils/formatFileSize'
import type { WorkspaceImportJob } from '@nosdesk/core/services/workspaceImportService'

import WorkspaceImportTiles from './WorkspaceImportTiles.vue'
import {
  NOT_MOVING,
  archiveName,
  areaLines,
  attentionLines,
  filterPeople,
  hostOf,
  offLines,
  peopleGroups,
  personOutcome,
  queuedMail,
  regeneratedTokens,
  tiles,
  typedMatches,
  worthALook,
  type PeopleFilter,
} from './workspaceImportModel'

const props = defineProps<{
  job: WorkspaceImportJob
  workspace: string
  confirming: boolean
}>()

const password = defineModel<string>('password', { required: true })

const emit = defineEmits<{
  (e: 'back'): void
  (e: 'confirm'): void
}>()

const fluent = useFluent()
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args)

const lang = typeof document === 'undefined' ? undefined : document.documentElement.lang || undefined
const list = (items: string[]) =>
  typeof Intl.ListFormat === 'function' ? new Intl.ListFormat(lang, { type: 'conjunction' }).format(items) : items.join(', ')

const source = computed(() => props.job.report?.source)
const plan = computed(() => props.job.report?.plan)
const name = computed(() => archiveName(props.job))
const counts = computed(() => tiles(plan.value))
const areas = computed(() => areaLines(plan.value))
const people = computed(() => plan.value?.people ?? [])
const groups = computed(() => peopleGroups(people.value))
const off = computed(() => offLines(plan.value))
const tokens = computed(() => regeneratedTokens(plan.value))
const unsent = computed(() => queuedMail(plan.value))
const attention = computed(() => attentionLines(plan.value))
const ssoOff = computed(() => off.value.some((l) => l.id === 'workspace_identity_providers'))

const exported = computed(() => {
  const d = source.value?.created_at ? new Date(source.value.created_at) : null
  return d && !Number.isNaN(d.getTime()) ? d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' }) : ''
})
const origin = computed(() => hostOf(source.value?.origin))

const filter = ref<PeopleFilter>('staff')
const staffCount = computed(() => filterPeople(people.value, 'staff').length)
const requesterCount = computed(() => filterPeople(people.value, 'requesters').length)
const attentionCount = computed(() => people.value.filter(worthALook).length)
const rows = computed(() => filterPeople(people.value, filter.value))
const requesters = computed(() => {
  const r = filterPeople(people.value, 'requesters')
  return {
    created: r.filter((p) => p.outcome === 'created').length,
    mapped: r.filter((p) => p.outcome === 'mapped').length,
  }
})

/** An area's counts as one sentence-case line. */
const areaText = (line: { counts: { key: string; count: number }[] }) => {
  const text = list(line.counts.map((c) => t(c.key, { count: c.count })))
  return text.charAt(0).toLocaleUpperCase(lang) + text.slice(1)
}

const OUTCOME_CLASS = { you: 'text-accent font-medium', warn: 'text-status-warning', faint: 'text-tertiary', plain: 'text-secondary' }

// ---- jump row ----

const SECTIONS = computed(() => [
  { id: 'across', label: t('workspace-import-jump-across') },
  { id: 'people', label: t('workspace-import-jump-people'), count: counts.value.people },
  { id: 'signin', label: t('workspace-import-jump-signin') },
  { id: 'replaces', label: t('workspace-import-jump-replaces') },
  { id: 'off', label: t('workspace-import-jump-off'), count: off.value.length },
  { id: 'not', label: t('workspace-import-jump-not') },
  { id: 'attention', label: t('workspace-import-jump-attention'), count: attention.value.length, warn: attention.value.length > 0 },
])

function jump(id: string) {
  document.getElementById(`workspace-preview-${id}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
}

// ---- confirm ----

const showConfirm = ref(false)
const typed = ref('')
const needsPassword = computed(() => props.job.archive.sealed && !password.value)
const canConfirm = computed(() => typedMatches(typed.value, props.workspace) && !needsPassword.value)

function openConfirm() {
  typed.value = ''
  showConfirm.value = true
}

function confirm() {
  if (!canConfirm.value) return
  showConfirm.value = false
  emit('confirm')
}
</script>

<template>
  <section class="flex flex-col gap-6 min-w-0" data-test="workspace-preview">
    <Callout severity="info" icon="archive">
      <template #header>
        <span class="font-medium text-primary break-words" data-test="workspace-preview-source">
          {{ origin
            ? $t('workspace-import-preview-source', { name, date: exported, origin })
            : $t('workspace-import-preview-source-no-origin', { name, date: exported }) }}
        </span>
      </template>
      <div class="flex flex-col gap-1.5 px-4 py-3">
        <p class="text-sm text-secondary">
          {{ $t('workspace-import-preview-lead', { workspace, people: counts.people }) }}
        </p>
        <p class="text-xs text-tertiary">
          {{ $t('workspace-import-preview-made-by', { version: source?.nosdesk_version ?? '', size: formatFileSize(job.archive.size) }) }}
        </p>
      </div>
    </Callout>

    <WorkspaceImportTiles
      :tiles="[
        { label: $t('workspace-import-tile-tickets'), value: counts.tickets, sub: $t('workspace-import-n-comments', { count: counts.comments }) },
        { label: $t('workspace-import-tile-pages'), value: counts.pages, sub: $t('workspace-import-n-collections', { count: counts.collections }) },
        { label: $t('workspace-import-tile-assets'), value: counts.assets, sub: $t('workspace-import-n-projects', { count: counts.projects }) },
        { label: $t('workspace-import-tile-people'), value: counts.people, sub: $t('workspace-import-tile-people-sub', { staff: counts.staff, requesters: counts.requesters }) },
      ]"
    />

    <nav
      class="sticky top-0 z-10 -my-2 py-2 bg-app flex gap-1.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      :aria-label="$t('workspace-import-jump-label')"
      data-test="workspace-preview-jump"
    >
      <button
        v-for="s in SECTIONS"
        :key="s.id"
        type="button"
        class="shrink-0 text-xs px-2.5 py-1 rounded-full border bg-surface text-secondary hover:text-primary hover:border-strong"
        :class="s.warn ? 'border-status-warning/50' : 'border-default'"
        @click="jump(s.id)"
      >
        {{ s.label }}<span v-if="s.count" class="text-tertiary ml-1">{{ s.count.toLocaleString() }}</span>
      </button>
    </nav>

    <!-- What comes across -->
    <SectionCard id="workspace-preview-across" content-padding="" class="scroll-mt-14">
      <template #title>
        {{ $t('workspace-import-across-heading') }}
        <span class="font-medium text-tertiary"> · {{ $t('workspace-import-n-areas', { count: areas.length }) }}</span>
      </template>
      <ul class="flex flex-col divide-y divide-default" data-test="workspace-preview-areas">
        <li v-for="a in areas" :key="a.id" class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5 text-sm" :data-area="a.id">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t(a.label) }}</span>
          <div class="flex-1 min-w-0">
            <p class="text-primary">{{ areaText(a) }}</p>
            <p v-if="a.note" class="text-xs text-tertiary mt-0.5">{{ $t(a.note) }}</p>
          </div>
        </li>
      </ul>
    </SectionCard>

    <!-- People -->
    <SectionCard id="workspace-preview-people" content-padding="" class="scroll-mt-14">
      <template #title>
        {{ $t('workspace-import-people-heading') }}
        <span class="font-medium text-tertiary"> · {{ $t('workspace-import-people-by-email', { count: counts.people }) }}</span>
      </template>
      <div class="flex flex-col gap-3 p-4">
        <div class="grid grid-cols-2 sm:grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-2" data-test="workspace-people-groups">
          <div v-for="g in groups" :key="g.key" class="flex flex-col gap-0.5 rounded-lg border border-default bg-surface-alt px-2.5 py-2 min-w-0">
            <span class="text-lg font-semibold text-primary">{{ g.count.toLocaleString() }}</span>
            <span class="text-xs text-secondary">{{ $t(g.key, { count: g.count }) }}</span>
          </div>
        </div>
        <p class="text-xs-plus text-secondary">{{ $t('workspace-import-people-note') }}</p>
      </div>
      <div class="flex flex-wrap items-center gap-2 px-3 pb-2.5">
        <div class="inline-flex border border-strong rounded-lg overflow-hidden ml-auto" role="group" data-test="workspace-people-filter">
          <button
            v-for="f in (['staff', 'attention', 'requesters'] as const)"
            :key="f"
            type="button"
            class="px-2.5 py-1 text-xs border-l border-default first:border-l-0"
            :class="filter === f ? 'bg-surface-hover text-primary font-medium' : 'bg-surface text-secondary'"
            :aria-pressed="filter === f"
            :data-filter="f"
            @click="filter = f"
          >
            {{ $t(`workspace-import-filter-${f}`, { count: f === 'staff' ? staffCount : f === 'requesters' ? requesterCount : attentionCount }) }}
          </button>
        </div>
      </div>
      <ul class="flex flex-col divide-y divide-default border-t border-default max-h-[32rem] overflow-y-auto" data-test="workspace-people">
        <li
          v-for="p in rows"
          :key="p.archive_uuid"
          class="grid grid-cols-[minmax(0,1fr)_auto] sm:grid-cols-[minmax(0,1.2fr)_minmax(0,1.4fr)_6rem_minmax(0,1.4fr)] gap-x-3 gap-y-0.5 items-center px-3 py-2 text-sm"
          data-test="workspace-person"
        >
          <span class="font-medium text-primary truncate">
            {{ p.name || $t('workspace-import-person-unnamed') }}
            <span v-if="p.outcome === 'merged_into_runner'" class="ml-1 text-2xs px-1.5 py-0.5 rounded bg-surface-hover text-secondary font-normal">{{ $t('workspace-import-person-you') }}</span>
          </span>
          <span class="row-start-2 sm:row-start-auto text-xs text-tertiary truncate">
            <template v-if="p.email">{{ p.email }}</template>
            <i v-else>{{ $t('workspace-import-person-no-email') }}</i>
          </span>
          <span class="row-start-2 col-start-2 sm:row-start-auto sm:col-start-auto justify-self-end sm:justify-self-start text-xs text-secondary truncate">
            {{ p.archive_role ?? '' }}<template v-if="p.disabled"> · {{ $t('workspace-import-person-removed') }}</template>
          </span>
          <span class="col-start-2 row-start-1 sm:col-start-auto sm:row-start-auto justify-self-end sm:justify-self-start text-right sm:text-left text-xs" :class="OUTCOME_CLASS[personOutcome(p).tone]" data-test="workspace-person-outcome">
            {{ $t(personOutcome(p).key, personOutcome(p).args) }}
          </span>
        </li>
        <li v-if="rows.length === 0" class="px-3 py-3 text-sm text-secondary">{{ $t('workspace-import-people-none') }}</li>
      </ul>
      <p v-if="requesterCount > 0" class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary" data-test="workspace-requesters-line">
        {{ $t('workspace-import-requesters-line', { created: requesters.created, mapped: requesters.mapped }) }}
      </p>
    </SectionCard>

    <!-- Sign-in -->
    <SectionCard id="workspace-preview-signin" content-padding="" class="scroll-mt-14">
      <template #title>{{ $t('workspace-import-signin-heading') }}</template>
      <ul class="flex flex-col divide-y divide-default text-sm" data-test="workspace-signin">
        <li class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-signin-you') }}</span>
          <p class="flex-1 min-w-0 text-secondary">{{ $t('workspace-import-signin-you-detail') }}</p>
        </li>
        <li class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-signin-staff') }}</span>
          <p class="flex-1 min-w-0 text-secondary break-words">{{ $t('workspace-import-signin-staff-detail') }}</p>
        </li>
        <li class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-signin-requesters') }}</span>
          <p class="flex-1 min-w-0 text-secondary">{{ $t('workspace-import-signin-requesters-detail') }}</p>
        </li>
        <li class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-signin-secrets') }}</span>
          <p class="flex-1 min-w-0 text-secondary">
            {{ $t('workspace-import-signin-secrets-detail') }}
            <template v-if="ssoOff"> {{ $t('workspace-import-signin-sso-off') }}</template>
          </p>
        </li>
      </ul>
    </SectionCard>

    <!-- What this workspace gives up -->
    <SectionCard id="workspace-preview-replaces" content-padding="" class="scroll-mt-14">
      <template #title>{{ $t('workspace-import-replaces-heading') }}</template>
      <ul class="flex flex-col divide-y divide-default text-sm" data-test="workspace-replaces">
        <li class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-replaces-config') }}</span>
          <div class="flex-1 min-w-0">
            <p class="text-primary">{{ $t('workspace-import-replaces-config-detail') }}</p>
            <p class="text-xs text-tertiary mt-0.5">{{ $t('workspace-import-replaces-config-note') }}</p>
          </div>
        </li>
        <li class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-replaces-name') }}</span>
          <p class="flex-1 min-w-0 text-primary break-words">{{ $t('workspace-import-replaces-name-detail', { workspace, archive: name }) }}</p>
        </li>
        <li class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-replaces-members') }}</span>
          <p class="flex-1 min-w-0 text-primary">{{ $t('workspace-import-replaces-members-detail') }}</p>
        </li>
      </ul>
    </SectionCard>

    <!-- Arrives switched off -->
    <SectionCard id="workspace-preview-off" content-padding="" class="scroll-mt-14">
      <template #title>
        {{ $t('workspace-import-off-heading') }}
        <span v-if="off.length" class="font-medium text-tertiary"> · {{ $t('workspace-import-n-integrations', { count: off.length }) }}</span>
      </template>
      <ul class="flex flex-col divide-y divide-default text-sm" data-test="workspace-off">
        <li v-for="l in off" :key="l.id" class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5" :data-off="l.id">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t(l.label, { count: l.count }) }}</span>
          <div class="flex-1 min-w-0">
            <p class="text-primary">{{ $t(l.detail, { count: l.count }) }}</p>
            <p class="text-xs text-tertiary mt-0.5">{{ $t(l.hint, { count: l.count }) }}</p>
          </div>
        </li>
        <li v-if="tokens > 0" class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5" data-off="tokens">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-off-guest-links') }}</span>
          <p class="flex-1 min-w-0 text-primary">{{ $t('workspace-import-off-guest-links-detail', { count: tokens }) }}</p>
        </li>
        <li v-if="off.length === 0 && tokens === 0" class="px-3 py-2.5 text-secondary">{{ $t('workspace-import-off-none') }}</li>
      </ul>
      <p class="px-3 py-2 border-t border-default bg-surface-alt text-xs text-tertiary">{{ $t('workspace-import-off-rule') }}</p>
    </SectionCard>

    <!-- Doesn't move -->
    <SectionCard id="workspace-preview-not" content-padding="" class="scroll-mt-14">
      <template #title>{{ $t('workspace-import-not-heading') }}</template>
      <ul class="flex flex-col divide-y divide-default text-sm" data-test="workspace-not">
        <li v-for="n in NOT_MOVING" :key="n.label" class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t(n.label) }}</span>
          <p class="flex-1 min-w-0 text-primary">{{ $t(n.detail) }}</p>
        </li>
        <li v-if="unsent > 0" class="flex flex-col sm:flex-row gap-1 sm:gap-3 px-3 py-2.5" data-not="mail">
          <span class="sm:w-36 shrink-0 font-medium text-primary">{{ $t('workspace-import-not-mail') }}</span>
          <p class="flex-1 min-w-0 text-primary">{{ $t('workspace-import-not-mail-detail', { count: unsent }) }}</p>
        </li>
      </ul>
    </SectionCard>

    <!-- Needs attention -->
    <SectionCard id="workspace-preview-attention" content-padding="" class="scroll-mt-14">
      <template #leading>
        <Icon v-if="attention.length" name="warning" size="sm" class="text-status-warning" />
      </template>
      <template #title>
        {{ $t('workspace-import-attention-heading') }}
        <span v-if="attention.length" class="font-medium text-tertiary"> · {{ $t('workspace-import-n-items', { count: attention.length }) }}</span>
      </template>
      <ul class="flex flex-col divide-y divide-default text-sm" data-test="workspace-attention">
        <li v-for="(a, i) in attention" :key="i" class="px-3 py-2.5 text-primary break-words">{{ $t(a.key, a.args) }}</li>
        <li v-if="attention.length === 0" class="px-3 py-2.5 text-secondary">{{ $t('workspace-import-attention-none') }}</li>
      </ul>
    </SectionCard>

    <div class="sticky bottom-0 z-10 -mx-4 sm:mx-0 px-4 sm:px-0 py-3 bg-app border-t border-default flex flex-wrap items-center gap-3">
      <Button variant="ghost" data-test="workspace-back" @click="emit('back')">{{ $t('workspace-import-back-to-archive') }}</Button>
      <span class="basis-full sm:basis-auto order-last sm:order-none text-xs text-tertiary">{{ $t('workspace-import-preview-reload-note') }}</span>
      <Button class="ml-auto" :loading="confirming" data-test="workspace-import-open-confirm" @click="openConfirm">
        {{ $t('workspace-import-import-button', { name }) }}
      </Button>
    </div>

    <ConfirmModal
      :show="showConfirm"
      variant="info"
      :title="$t('workspace-import-confirm-title', { name, workspace })"
      :description="$t('workspace-import-confirm-subtitle')"
      message=""
      :confirm-label="$t('workspace-import-import-button', { name })"
      :confirm-disabled="!canConfirm"
      :loading="confirming"
      @close="showConfirm = false"
      @confirm="confirm"
    >
      <template #body>
        <ul class="flex flex-col gap-2 text-sm text-secondary" data-test="workspace-confirm-stakes">
          <li class="flex gap-2.5"><Icon name="info" size="sm" class="shrink-0 mt-0.5 text-tertiary" />
            <span>{{ $t('workspace-import-confirm-fills', { tickets: counts.tickets, pages: counts.pages, assets: counts.assets, people: counts.people }) }}</span></li>
          <li class="flex gap-2.5"><Icon name="info" size="sm" class="shrink-0 mt-0.5 text-tertiary" />
            <span>{{ $t('workspace-import-confirm-replaces') }}</span></li>
          <li class="flex gap-2.5"><Icon name="info" size="sm" class="shrink-0 mt-0.5 text-tertiary" />
            <span>{{ $t('workspace-import-confirm-off') }}</span></li>
          <li class="flex gap-2.5"><Icon name="info" size="sm" class="shrink-0 mt-0.5 text-tertiary" />
            <span>{{ $t('workspace-import-confirm-reload') }}</span></li>
          <li class="flex gap-2.5"><Icon name="warning" size="sm" class="shrink-0 mt-0.5 text-status-error" />
            <span>{{ $t('workspace-import-confirm-reset') }}</span></li>
        </ul>
        <FormInput
          v-if="job.archive.sealed"
          v-model="password"
          type="password"
          size="sm"
          autocomplete="off"
          :label="$t('workspace-import-password-label')"
          :description="$t('workspace-import-confirm-password-hint')"
          data-test="workspace-confirm-password"
        />
        <div class="flex flex-col gap-1.5">
          <label for="workspace-confirm-name" class="text-xs font-medium text-secondary">
            {{ $t('workspace-import-type-to-confirm', { workspace }) }}
          </label>
          <input
            id="workspace-confirm-name"
            v-model="typed"
            type="text"
            autocomplete="off"
            autocapitalize="off"
            spellcheck="false"
            :placeholder="workspace"
            class="w-full px-3 py-2 text-sm rounded-lg border border-strong bg-surface text-primary focus:outline-none focus:border-accent focus:ring-2 focus:ring-accent/20"
            data-test="workspace-confirm-name"
            @keydown.enter="confirm"
          />
        </div>
      </template>
    </ConfirmModal>
  </section>
</template>
