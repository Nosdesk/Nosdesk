<script setup lang="ts">
import { computed, ref } from 'vue';
import { useFluent } from 'fluent-vue';
import { useQuery, useQueryCache } from '@pinia/colada';
import { useAuthStore } from '@/stores/auth';
import { useToastStore } from '@/stores/toast';
import { assetLoanKeys, assetLoanService } from '@/services/assetLoanService';
import { useSyncActions } from '@/composables/useSyncActions';
import Button from '@/components/common/Button.vue';
import TicketLoanRow from './TicketLoanRow.vue';
import IssueLoanerModal from './IssueLoanerModal.vue';
import type { AssetLoan } from '@nosdesk/core/types/asset';

const props = defineProps<{
  ticketId: number;
  requesterUuid?: string | null;
  /** Whether the ticket has any linked device. The loaner affordance is
   * shown on device/repair tickets (or any ticket that already has loans),
   * not on every ticket, to keep the sidebar uncluttered. */
  hasDevices?: boolean;
}>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const auth = useAuthStore();
const toast = useToastStore();
const canManage = computed(() => auth.isTechnician);

const queryCache = useQueryCache();
const loanKey = () => assetLoanKeys.forTicket(props.ticketId);
const loansQuery = useQuery({
  key: loanKey,
  query: () => assetLoanService.listByTicket(props.ticketId),
});
const loans = computed<AssetLoan[]>(() =>
  Array.isArray(loansQuery.data.value) ? loansQuery.data.value : [],
);
const loadError = computed(() => (loansQuery.error.value ? t('asset-loan-load-error') : ''));
const activeLoans = computed(() => loans.value.filter((l) => !l.returned_at));
const pastLoans = computed(() => loans.value.filter((l) => l.returned_at));

// Read surface shows whenever there are loans; the issue affordance is
// contextual (agents, on a ticket that has a device or an existing loan).
const canIssue = computed(() => canManage.value && (loans.value.length > 0 || !!props.hasDevices));
const visible = computed(() => loans.value.length > 0 || canIssue.value || !!loadError.value);

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
  } catch (e) {
    toast.error(e instanceof Error ? e.message : t('asset-loan-failed'));
  } finally {
    // Refetch the truth either way; a concurrent return just reconciles.
    await invalidate();
  }
}
</script>

<template>
  <div v-if="visible" class="flex flex-col gap-1 print:hidden">
    <div class="flex items-center justify-between gap-2 px-1">
      <h3 class="text-sm font-medium text-secondary">{{ $t('asset-loan-ticket-heading') }}</h3>
      <Button v-if="canIssue" size="sm" variant="ghost" icon="userPlus" @click="showIssue = true">
        {{ $t('asset-loan-loan-out') }}
      </Button>
    </div>

    <p v-if="loadError" class="text-xs text-status-error px-1">{{ loadError }}</p>

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
