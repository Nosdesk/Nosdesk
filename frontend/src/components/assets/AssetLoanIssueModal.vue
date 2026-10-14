<script setup lang="ts">
/**
 * Loan an asset out: borrower, loan period, an optional ticket and notes.
 * Opened from the asset page's state header.
 */
import { ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import IconButton from '@/components/common/IconButton.vue';
import Button from '@/components/common/Button.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import Icon from '@/components/common/Icon.vue';
import Modal from '@/components/Modal.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import TicketPickerModal from '@/components/ticketComponents/TicketPickerModal.vue';
import { assetLoanService } from '@nosdesk/core/services/assetLoanService';

const props = defineProps<{ show: boolean; assetId: number }>();
const emit = defineEmits<{ (e: 'close'): void; (e: 'issued'): void }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const showBorrowerPicker = ref(false);
const borrower = ref<{ uuid: string; name: string } | null>(null);
const loanedOn = ref('');
const dueBack = ref('');
const linkedTicket = ref<{ id: number; number: number; title: string } | null>(null);
const showTicketPicker = ref(false);
const notes = ref('');
const issueError = ref('');
const submitting = ref(false);

// A fresh form each time the dialog opens.
watch(
  () => props.show,
  (open) => {
    if (!open) return;
    borrower.value = null;
    loanedOn.value = new Date().toISOString().slice(0, 10);
    dueBack.value = '';
    linkedTicket.value = null;
    notes.value = '';
    issueError.value = '';
  },
  { immediate: true },
);

function onSelectBorrower(user: { uuid: string; name: string }) {
  showBorrowerPicker.value = false;
  if (user.uuid) borrower.value = { uuid: user.uuid, name: user.name };
}

function onSelectTicket(ticket: { id: number; number: number; title: string }) {
  linkedTicket.value = ticket;
}

async function submitIssue() {
  if (!borrower.value) return;
  submitting.value = true;
  issueError.value = '';
  try {
    await assetLoanService.issue(props.assetId, {
      borrower_user_uuid: borrower.value.uuid,
      loaned_at: loanedOn.value || null,
      due_back: dueBack.value || null,
      ticket_id: linkedTicket.value?.id ?? null,
      notes: notes.value.trim() || null,
    });
    emit('issued');
  } catch (e) {
    issueError.value = e instanceof Error ? e.message : t('asset-loan-failed');
  } finally {
    submitting.value = false;
  }
}
</script>

<template>

  <!-- Issue modal -->
  <Modal :show="show" :title="$t('asset-loan-issue-title')" size="md" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <!-- Borrower selector -->
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('asset-loan-borrower') }}</span>
        <button
          type="button"
          class="flex items-center gap-2 rounded-lg border border-default bg-surface px-3 py-2 text-left hover:border-strong transition-colors"
          @click="showBorrowerPicker = true"
        >
          <template v-if="borrower">
            <UserAvatar :uuid="borrower.uuid" size="xs" :show-name="false" :clickable="false" class="shrink-0" />
            <span class="text-sm text-primary truncate">{{ borrower.name }}</span>
          </template>
          <template v-else>
            <Icon name="userPlus" class="w-4 h-4 text-tertiary shrink-0" />
            <span class="text-sm text-tertiary">{{ $t('asset-loan-select-borrower') }}</span>
          </template>
          <Icon name="chevronDown" class="ml-auto w-4 h-4 text-tertiary shrink-0" />
        </button>
      </div>

      <!-- Loan period -->
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('asset-loan-period-label') }}</span>
        <DatePicker
          range
          v-model:start="loanedOn"
          v-model:end="dueBack"
          :aria-label="$t('asset-loan-period-label')"
          block
        />
        <p class="text-xs text-tertiary">{{ $t('asset-loan-period-hint') }}</p>
      </div>

      <!-- Linked ticket selector -->
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-tertiary uppercase tracking-wide">{{ $t('asset-loan-ticket-field') }}</span>
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="flex-1 min-w-0 flex items-center gap-2 rounded-lg border border-default bg-surface px-3 py-2 text-left hover:border-strong transition-colors"
            @click="showTicketPicker = true"
          >
            <template v-if="linkedTicket">
              <span class="text-xs font-mono text-tertiary shrink-0">#{{ linkedTicket.number }}</span>
              <span class="text-sm text-primary truncate">{{ linkedTicket.title || $t('ticket-picker-untitled') }}</span>
            </template>
            <template v-else>
              <Icon name="ticket" size="sm" class="text-tertiary shrink-0" />
              <span class="text-sm text-tertiary">{{ $t('asset-loan-ticket-link') }}</span>
            </template>
            <Icon v-if="!linkedTicket" name="chevronDown" class="ml-auto w-4 h-4 text-tertiary shrink-0" />
          </button>
          <IconButton
            :label="$t('asset-loan-ticket-clear')"
            icon="close"
            variant="ghost-danger"
            class="shrink-0"
            v-if="linkedTicket"
            @click="linkedTicket = null"
          />
        </div>
      </div>

      <FormTextarea v-model="notes" :label="$t('asset-loan-notes')" :rows="2" />

      <p v-if="issueError" class="text-sm text-status-error">{{ issueError }}</p>
    </div>
    <template #footer>
      <div class="modal-actions">
        <Button variant="secondary" @click="emit('close')">{{ $t('asset-loan-cancel') }}</Button>
        <Button :loading="submitting" :disabled="!borrower" @click="submitIssue">
          {{ $t('asset-loan-loan-out') }}
        </Button>
      </div>
    </template>
  </Modal>

  <!-- Borrower picker, layered over the issue modal -->
  <UserSelectionModal
    :show="showBorrowerPicker"
    scope="requester"
    :current-user-id="borrower?.uuid ?? null"
    @close="showBorrowerPicker = false"
    @select-user="onSelectBorrower"
  />

  <!-- Ticket picker (search existing or create new), over the issue modal -->
  <TicketPickerModal
    :show="showTicketPicker"
    allow-create
    @close="showTicketPicker = false"
    @select="onSelectTicket"
  />
</template>
