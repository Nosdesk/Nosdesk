<script setup lang="ts">
import { ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import Modal from '@/components/Modal.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import FormTextarea from '@/components/common/FormTextarea.vue';
import DatePicker from '@/components/common/DatePicker.vue';
import Icon from '@/components/common/Icon.vue';
import UserAvatar from '@/components/UserAvatar.vue';
import UserSelectionModal from '@/components/UserSelectionModal.vue';
import { getPaginatedAssets, type AssetPaginationParams } from '@/services/assetService';
import { assetLoanService } from '@nosdesk/core/services/assetLoanService';
import { useUsersDirectory } from '@/composables/useUsersDirectory';
import { metaForAssetStatus } from '@/utils/assetStatusMeta';
import type { Asset } from '@nosdesk/core/types/asset';

const props = defineProps<{
  show: boolean;
  ticketId: number;
  requesterUuid?: string | null;
}>();
const emit = defineEmits<{ (e: 'close'): void; (e: 'issued'): void }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const { getUserHandle } = useUsersDirectory();

const borrower = ref<{ uuid: string; name: string } | null>(null);
const showBorrowerPicker = ref(false);

const search = ref('');
const devices = ref<Asset[]>([]);
const loadingDevices = ref(false);
const selectedDevice = ref<Asset | null>(null);

const dueBack = ref('');
const notes = ref('');
const submitting = ref(false);
const errorMessage = ref('');
const today = new Date().toISOString().slice(0, 10);

let searchTimer: ReturnType<typeof setTimeout> | null = null;

watch(
  () => props.show,
  (show) => {
    if (!show) return;
    selectedDevice.value = null;
    search.value = '';
    dueBack.value = '';
    notes.value = '';
    errorMessage.value = '';
    // Default the borrower to the ticket's requester (overridable).
    borrower.value = props.requesterUuid
      ? { uuid: props.requesterUuid, name: getUserHandle(props.requesterUuid).user.value?.name ?? '' }
      : null;
    void loadDevices();
  },
);

async function loadDevices() {
  loadingDevices.value = true;
  try {
    // Only loanable spares: in service / in stock. on_loan devices are
    // excluded by the status filter.
    const params: AssetPaginationParams = {
      page: 1,
      pageSize: 20,
      search: search.value.trim() || undefined,
      status: 'in_stock,in_service',
    };
    const res = await getPaginatedAssets(params, 'loaner-picker');
    devices.value = res.data;
  } catch (e) {
    if (e instanceof Error && e.message === 'REQUEST_CANCELLED') return;
    devices.value = [];
  } finally {
    loadingDevices.value = false;
  }
}

// Debounced reload as the agent types. A watcher (not an `@input`
// listener) so it doesn't depend on FormInput forwarding the native event.
watch(search, () => {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(() => void loadDevices(), 300);
});

function onSelectBorrower(user: { uuid: string; name: string }) {
  showBorrowerPicker.value = false;
  if (user.uuid) borrower.value = { uuid: user.uuid, name: user.name };
}

async function submit() {
  if (!selectedDevice.value || !borrower.value) return;
  submitting.value = true;
  errorMessage.value = '';
  try {
    await assetLoanService.issue(selectedDevice.value.id, {
      borrower_user_uuid: borrower.value.uuid,
      due_back: dueBack.value || null,
      ticket_id: props.ticketId,
      notes: notes.value.trim() || null,
    });
    emit('issued');
    emit('close');
  } catch (e) {
    errorMessage.value = e instanceof Error ? e.message : t('asset-loan-failed');
  } finally {
    submitting.value = false;
  }
}
</script>

<template>
  <Modal :show="show" :title="$t('asset-loan-issue-title')" size="md" @close="emit('close')">
    <div class="flex flex-col gap-4">
      <!-- Device -->
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-tertiary">{{ $t('asset-loan-device') }}</span>
        <div
          v-if="selectedDevice"
          class="flex items-center justify-between gap-2 rounded-lg border border-default bg-surface px-3 py-2"
        >
          <span class="text-sm text-primary truncate">{{ selectedDevice.name }}</span>
          <button type="button" class="text-xs text-accent hover:underline" @click="selectedDevice = null">
            {{ $t('asset-loan-change') }}
          </button>
        </div>
        <template v-else>
          <FormInput
            v-model="search"
            :placeholder="$t('asset-loan-device-search')"
            :aria-label="$t('asset-loan-device-search')"
          />
          <div class="rounded-lg border border-default bg-surface max-h-48 overflow-y-auto divide-y divide-default">
            <div v-if="loadingDevices" class="px-3 py-3 text-xs text-tertiary text-center">
              {{ $t('asset-loan-loading') }}
            </div>
            <div v-else-if="devices.length === 0" class="px-3 py-3 text-xs text-tertiary text-center">
              {{ $t('asset-loan-no-loanable') }}
            </div>
            <button
              v-for="d in devices"
              :key="d.id"
              type="button"
              class="w-full flex items-center justify-between gap-2 px-3 py-2 text-left hover:bg-surface-hover transition-colors"
              @click="selectedDevice = d"
            >
              <span class="text-sm text-primary truncate">{{ d.name }}</span>
              <span class="text-xs text-tertiary whitespace-nowrap">
                {{ $t(metaForAssetStatus(d.status).labelKey) }}
              </span>
            </button>
          </div>
        </template>
      </div>

      <!-- Borrower (defaults to the ticket requester) -->
      <div class="flex flex-col gap-1.5">
        <span class="text-xs font-medium text-tertiary">{{ $t('asset-loan-borrower') }}</span>
        <button
          type="button"
          class="flex items-center gap-2 rounded-lg border border-default bg-surface px-3 py-2 text-left hover:bg-surface-hover transition-colors"
          @click="showBorrowerPicker = true"
        >
          <template v-if="borrower">
            <UserAvatar :uuid="borrower.uuid" size="xs" :clickable="false" />
            <span class="text-sm text-primary">{{ borrower.name || $t('asset-loan-unknown-borrower') }}</span>
          </template>
          <template v-else>
            <Icon name="userPlus" class="w-4 h-4 text-tertiary" />
            <span class="text-sm text-tertiary">{{ $t('asset-loan-select-borrower') }}</span>
          </template>
        </button>
      </div>

      <DatePicker v-model="dueBack" :label="$t('asset-loan-due-back-optional')" :min="today" />
      <FormTextarea v-model="notes" :label="$t('asset-loan-notes')" :rows="2" />
      <p v-if="errorMessage" class="text-sm text-status-error">{{ errorMessage }}</p>
    </div>
    <template #footer>
      <div class="modal-actions">
        <Button variant="secondary" @click="emit('close')">{{ $t('asset-loan-cancel') }}</Button>
        <Button :loading="submitting" :disabled="!selectedDevice || !borrower" @click="submit">
          {{ $t('asset-loan-loan-out') }}
        </Button>
      </div>
    </template>
  </Modal>

  <UserSelectionModal
    :show="showBorrowerPicker"
    :current-user-id="borrower?.uuid ?? null"
    @close="showBorrowerPicker = false"
    @select-user="onSelectBorrower"
  />
</template>
