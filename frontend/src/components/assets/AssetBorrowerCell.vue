<script setup lang="ts">
/**
 * The inventory list's Borrower cell: the open loan's borrower, a muted
 * "Loaned" when the borrower's account is gone, nothing when the asset
 * isn't out. The title gives the borrower and the loan dates.
 */
import { computed } from 'vue'
import { useFluent } from 'fluent-vue'
import type { AssetCurrentLoan } from '@nosdesk/core/types/asset'
import { UserAvatarCell } from '@/components/common/cells'
import { borrowerCell } from '@/utils/assetLoanStatusColumns'

const props = defineProps<{ loan: AssetCurrentLoan | null | undefined }>()

const fluent = useFluent()
const cell = computed(() => borrowerCell((key, args) => fluent.$t(key, args), props.loan))
</script>

<template>
  <!-- `xs` avatar: a compact row's content box is 24px. -->
  <div v-if="cell.kind === 'user'" class="min-w-0" :title="cell.title">
    <UserAvatarCell :user-id="cell.uuid" :user-name="cell.name" size="xs" :show-name="true" />
  </div>
  <span v-else-if="cell.kind === 'unknown'" class="text-xs text-tertiary truncate" :title="cell.title">
    {{ $t('assets-list-borrower-unknown') }}
  </span>
</template>
