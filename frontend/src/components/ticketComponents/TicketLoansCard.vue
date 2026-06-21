<script setup lang="ts">
import { computed, ref } from 'vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { useAuthStore } from '@/stores/auth';
import { assetLoanService } from '@/services/assetLoanService';
import { useSyncActions } from '@/composables/useSyncActions';
import Button from '@/components/common/Button.vue';
import TicketLoanRow from './TicketLoanRow.vue';
import IssueLoanerModal from './IssueLoanerModal.vue';
import type { AssetLoan } from '@/types/asset';

const props = defineProps<{ ticketId: number; requesterUuid?: string | null }>();

const auth = useAuthStore();
const canManage = computed(() => auth.isTechnician);

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

const showIssue = ref(false);

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
  <div v-if="loans.length > 0 || canManage" class="flex flex-col gap-1 print:hidden">
    <div class="flex items-center justify-between gap-2 px-1">
      <h3 class="text-sm font-medium text-secondary">{{ $t('asset-loan-ticket-heading') }}</h3>
      <Button v-if="canManage" size="sm" variant="ghost" icon="userPlus" @click="showIssue = true">
        {{ $t('asset-loan-loan-out') }}
      </Button>
    </div>

    <div
      v-if="loans.length > 0"
      class="rounded-lg border border-default bg-surface-alt px-3 divide-y divide-default"
    >
      <TicketLoanRow
        v-for="loan in activeLoans"
        :key="loan.id"
        :loan="loan"
        :can-return="canManage"
        @return="returnLoan(loan)"
      />
      <TicketLoanRow v-for="loan in pastLoans" :key="loan.id" :loan="loan" />
    </div>

    <IssueLoanerModal
      :show="showIssue"
      :ticket-id="ticketId"
      :requester-uuid="requesterUuid"
      @close="showIssue = false"
      @issued="invalidate"
    />
  </div>
</template>
