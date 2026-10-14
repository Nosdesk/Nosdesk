<script setup lang="ts">
/**
 * The inventory list's Used by cell. While an asset is on loan it shows the
 * borrower, marked with the on-loan glyph (red once overdue), since they're
 * the one who has it; otherwise the assigned user. The title gives the loan
 * dates and who the asset is assigned to.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import type { AssetCurrentLoan } from '@nosdesk/core/types/asset'
import Icon from '@/components/common/Icon.vue'
import { UserAvatarCell } from '@/components/common/cells'
import { usedByCell } from '@/utils/assetLoanStatusColumns'

const props = defineProps<{
  primaryUser: { uuid: string; name: string; avatar_thumb?: string | null; avatar_url?: string | null } | null | undefined
  loan: AssetCurrentLoan | null | undefined
}>()

const fluent = useFluent()
const cell = computed(() =>
  usedByCell((key, args) => fluent.$t(key, args), props.primaryUser, props.loan),
)
</script>

<template>
  <!-- `xs` avatars: a compact row's content box is 24px. -->
  <div v-if="cell.kind === 'loan'" class="flex items-center gap-1.5 min-w-0" :title="cell.title">
    <Icon
      name="team"
      size="xs"
      class="shrink-0"
      :class="cell.overdue ? 'text-status-error' : 'text-accent'"
    />
    <span class="sr-only">{{ cell.title }}</span>
    <UserAvatarCell
      v-if="cell.uuid"
      :user-id="cell.uuid"
      :user-name="cell.name ?? undefined"
      size="xs"
      :show-name="true"
      aria-hidden="true"
    />
    <span v-else class="text-xs text-tertiary truncate" aria-hidden="true">
      {{ $t('assets-list-borrower-unknown') }}
    </span>
  </div>
  <UserAvatarCell
    v-else-if="cell.kind === 'user'"
    :user-id="cell.uuid"
    :user-name="cell.name"
    :avatar="primaryUser?.avatar_thumb || primaryUser?.avatar_url"
    size="xs"
    :show-name="true"
  />
  <span v-else class="text-xs text-tertiary">{{ $t('assets-list-unassigned') }}</span>
</template>
