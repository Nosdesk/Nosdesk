<script setup lang="ts">
import { computed } from 'vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { useAuthStore } from '@/stores/auth';
import { assetLoanService } from '@/services/assetLoanService';
import { useSyncActions } from '@/composables/useSyncActions';
import TicketLoanRow from './TicketLoanRow.vue';
import type { AssetLoan } from '@/types/asset';

const props = defineProps<{ ticketId: number }>();

const auth = useAuthStore();
const canReturn = computed(() => auth.isTechnician);

const queryCache = useQueryCache();
const loanKey = () => ['ticket-loans', props.ticketId] as const;
const loansQuery = useQuery({
  key: loanKey,
  query: () => assetLoanService.listByTicket(props.ticketId),
});
const loans = computed<AssetLoan[]>(() =>
  Array.isArray(loansQuery.data.value) ? loansQuery.data.value : [],
);
const activeLoans = computed(() => loans.value.filter((l) => !l.returned_at));
const pastLoans = computed(() => loans.value.filter((l) => l.returned_at));

function invalidate() {
  return queryCache.invalidateQueries({ key: loanKey() });
}

useSyncActions(
  (actions) => {
    if (
      actions.some((a) => {
        const data = a.data as { ticket_id?: number };
        return data.ticket_id === props.ticketId;
      })
    ) {
      void invalidate();
    }
  },
  { aggregates: ['asset_loan'], debounceMs: 250 },
);

async function returnLoan(loan: AssetLoan) {
  try {
    await assetLoanService.returnLoan(loan.asset_id, loan.id, {});
  } finally {
    // Refetch the truth either way; a 409 (already returned elsewhere)
    // just reconciles the row.
    await invalidate();
  }
}
</script>

<template>
  <div v-if="loans.length > 0" class="flex flex-col gap-1 print:hidden">
    <h3 class="text-sm font-medium text-secondary px-1">{{ $t('asset-loan-ticket-heading') }}</h3>
    <div class="rounded-lg border border-default bg-surface-alt px-3 divide-y divide-default">
      <TicketLoanRow
        v-for="loan in activeLoans"
        :key="loan.id"
        :loan="loan"
        :can-return="canReturn"
        @return="returnLoan(loan)"
      />
      <TicketLoanRow v-for="loan in pastLoans" :key="loan.id" :loan="loan" />
    </div>
  </div>
</template>
