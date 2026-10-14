<script setup lang="ts">
/**
 * An asset's loan ledger: the open loan with its notes and ticket, then the
 * returned ones. Renders nothing until the asset has been loaned. Issuing and
 * returning happen in the page's state header.
 */
import { computed } from 'vue';
import { RouterLink } from 'vue-router';
import { useFluent } from 'fluent-vue';
import SectionCard from '@/components/common/SectionCard.vue';
import StatusPill from '@/components/common/StatusPill.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import { queryLoadError } from '@/composables/queryLoadError';
import { useAssetLoans } from '@/composables/useAssetLoans';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { formatCompactDate, formatRelativeTime } from '@nosdesk/core/utils/dateUtils';
import { loanDue } from './loanDue';
import type { AssetLoan } from '@nosdesk/core/types/asset';
import { numberForTicketId } from '@/composables/useTicketNumberLookup';
import { ticketPath } from '@/utils/ticketNumbers';

const props = defineProps<{ assetId: number }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const { getUserHandle } = useUsersDirectory();

const { query, loans, activeLoan, pastLoans } = useAssetLoans(() => props.assetId);
const loadError = queryLoadError(query, () => t('asset-loan-load-error'));

function borrowerName(loan: AssetLoan): string {
  if (!loan.borrower_user_uuid) return t('asset-loan-deleted-borrower');
  return getUserHandle(loan.borrower_user_uuid).user.value?.name ?? t('asset-loan-unknown-borrower');
}

function relative(date: string | null | undefined): string {
  return date ? formatRelativeTime(date, { addSuffix: true }) : '';
}

/** Range "loaned -> returned" for a finished loan, used in the history list. */
function loanRange(loan: AssetLoan): string {
  return t('asset-loan-range', {
    from: formatCompactDate(loan.loaned_at),
    to: loan.returned_at ? formatCompactDate(loan.returned_at) : '',
  });
}

/** Due-date pill for the open loan. `null` when the loan is open-ended. */
const activeDue = computed(() => (activeLoan.value ? loanDue(activeLoan.value, t) : null));
</script>

<template>
  <SectionCard v-if="loans.length > 0 || loadError" content-padding="p-4">
    <template #title>{{ $t('asset-loan-heading') }}</template>
    <div class="flex flex-col gap-3">
      <p v-if="loadError" class="text-xs text-status-error">{{ loadError }}</p>

      <!-- Active loan: one vertically-centered identity row (avatar · borrower+meta ·
           due), with notes dropped beneath and aligned to the name. -->
      <div
        v-if="activeLoan"
        class="rounded-lg border border-default bg-surface-alt p-3 flex flex-col gap-2.5"
      >
        <div class="flex items-center gap-3">
          <UserAvatar
            :uuid="activeLoan.borrower_user_uuid"
            size="md"
            :show-name="false"
            :clickable="false"
            class="shrink-0"
          />
          <div class="min-w-0 flex-1">
            <p class="text-sm font-semibold text-primary truncate leading-tight">
              {{ borrowerName(activeLoan) }}
            </p>
            <div class="flex items-center gap-x-1.5 text-xs text-tertiary leading-tight mt-0.5 overflow-hidden">
              <span class="truncate">{{ $t('asset-loan-fact-loaned') }} {{ formatCompactDate(activeLoan.loaned_at) }}</span>
              <span aria-hidden="true" class="shrink-0">·</span>
              <span class="shrink-0">{{ relative(activeLoan.loaned_at) }}</span>
              <template v-if="activeLoan.ticket_id && numberForTicketId(activeLoan.ticket_id) !== undefined">
                <span aria-hidden="true" class="shrink-0">·</span>
                <RouterLink
                  :to="ticketPath({ id: activeLoan.ticket_id, number: numberForTicketId(activeLoan.ticket_id) })"
                  class="shrink-0 text-accent hover:underline"
                >{{ $t('asset-loan-ticket', { id: numberForTicketId(activeLoan.ticket_id) ?? '' }) }}</RouterLink>
              </template>
            </div>
          </div>
          <StatusPill
            v-if="activeDue"
            :label="activeDue.label"
            :tone="activeDue.tone"
            size="sm"
            class="shrink-0"
          />
        </div>

        <!-- Notes, aligned to the borrower name (md avatar 2rem + gap-3 0.75rem) -->
        <p
          v-if="activeLoan.notes"
          class="text-sm text-secondary whitespace-pre-line break-words pl-11"
        >
          {{ activeLoan.notes }}
        </p>
      </div>

      <!-- Returned loans -->
      <div v-if="pastLoans.length > 0" class="flex flex-col">
        <div
          v-for="loan in pastLoans"
          :key="loan.id"
          class="flex items-center gap-2.5 py-2 border-t border-subtle first:border-t-0"
        >
          <UserAvatar
            :uuid="loan.borrower_user_uuid"
            size="xs"
            :show-name="false"
            :clickable="false"
            class="shrink-0"
          />
          <div class="min-w-0 flex-1">
            <p class="text-sm text-primary truncate">{{ borrowerName(loan) }}</p>
            <p class="text-xs text-tertiary truncate">{{ loanRange(loan) }}</p>
          </div>
          <span class="text-xs text-tertiary whitespace-nowrap shrink-0">
            {{ relative(loan.returned_at) }}
          </span>
        </div>
      </div>
    </div>
  </SectionCard>
</template>
