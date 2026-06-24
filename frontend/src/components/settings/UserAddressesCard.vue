<script setup lang="ts">
/** A user's addresses (vCard ADR): typed, multi-valued, structured. Directory-
 *  synced rows (source set) are read-only. Mirrors UserEmailsCard. */
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import SectionCard from '@/components/common/SectionCard.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import Icon from '@/components/common/Icon.vue';
import { extractErrorMessage } from '@/utils/errors';
import { useToastStore } from '@/stores/toast';
import {
  listUserAddresses,
  addUserAddress,
  updateUserAddress,
  deleteUserAddress,
  type UserAddress,
} from '@/services/userContactService';

const props = defineProps<{ uuid: string; editable: boolean }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

const addresses = ref<UserAddress[]>([]);
const showAddForm = ref(false);
const saving = ref(false);
const pendingDelete = ref<UserAddress | null>(null);

const TYPES = ['work', 'home', 'other'];

function blankDraft() {
  return {
    address_type: 'work',
    is_primary: false,
    street: '',
    city: '',
    region: '',
    postal_code: '',
    country: '',
  };
}
const draft = ref(blankDraft());

async function load(): Promise<void> {
  try {
    addresses.value = await listUserAddresses(props.uuid);
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-addresses-error-load')));
  }
}
watch(() => props.uuid, load, { immediate: true });

function resetDraft(): void {
  draft.value = blankDraft();
  showAddForm.value = false;
}

const draftEmpty = computed(
  () =>
    !draft.value.street?.trim() &&
    !draft.value.city?.trim() &&
    !draft.value.region?.trim() &&
    !draft.value.postal_code?.trim() &&
    !draft.value.country?.trim(),
);

async function add(): Promise<void> {
  if (draftEmpty.value) return;
  saving.value = true;
  try {
    await addUserAddress(props.uuid, { ...draft.value });
    resetDraft();
    await load();
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-addresses-error-save')));
  } finally {
    saving.value = false;
  }
}

async function setPrimary(a: UserAddress): Promise<void> {
  try {
    await updateUserAddress(props.uuid, a.id, {
      address_type: a.address_type,
      is_primary: true,
      street: a.street,
      city: a.city,
      region: a.region,
      postal_code: a.postal_code,
      country: a.country,
      label: a.label,
    });
    await load();
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-addresses-error-save')));
  }
}

async function doDelete(): Promise<void> {
  const target = pendingDelete.value;
  pendingDelete.value = null;
  if (!target) return;
  try {
    await deleteUserAddress(props.uuid, target.id);
    await load();
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-addresses-error-delete')));
  }
}

function formatAddress(a: UserAddress): string {
  return [a.street, a.city, a.region, a.postal_code, a.country].filter(Boolean).join(', ');
}
</script>

<template>
  <SectionCard content-padding="p-4">
    <template #title>{{ t('user-addresses-title') }}</template>
    <template #headerActions>
      <Button v-if="editable && !showAddForm" size="sm" icon="add" @click="showAddForm = true">
        {{ t('user-addresses-add') }}
      </Button>
    </template>

    <div
      v-if="showAddForm && editable"
      class="mb-3 p-3 bg-surface-alt rounded-lg border border-subtle flex flex-col gap-2"
    >
      <div class="flex items-center gap-2">
        <select
          v-model="draft.address_type"
          class="bg-surface border border-default rounded-lg px-2 py-1.5 text-sm text-primary focus:outline-none focus:border-accent"
        >
          <option v-for="ty in TYPES" :key="ty" :value="ty">{{ t(`user-addresses-type-${ty}`) }}</option>
        </select>
        <label class="flex items-center gap-1.5 text-sm text-secondary whitespace-nowrap">
          <input v-model="draft.is_primary" type="checkbox" /> {{ t('user-addresses-primary') }}
        </label>
      </div>
      <FormInput v-model="draft.street" :placeholder="t('user-addresses-field-street')" size="sm" />
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
        <FormInput v-model="draft.city" :placeholder="t('user-addresses-field-city')" size="sm" />
        <FormInput v-model="draft.region" :placeholder="t('user-addresses-field-region')" size="sm" />
        <FormInput v-model="draft.postal_code" :placeholder="t('user-addresses-field-postal')" size="sm" />
        <FormInput v-model="draft.country" :placeholder="t('user-addresses-field-country')" size="sm" />
      </div>
      <div class="flex justify-end gap-2">
        <Button size="sm" :loading="saving" :disabled="draftEmpty" @click="add">
          {{ t('user-addresses-add') }}
        </Button>
        <Button variant="secondary" size="sm" @click="resetDraft">{{ t('common-cancel') }}</Button>
      </div>
    </div>

    <p v-if="addresses.length === 0" class="text-sm text-tertiary py-2">{{ t('user-addresses-empty') }}</p>

    <div v-else class="flex flex-col gap-2">
      <div
        v-for="a in addresses"
        :key="a.id"
        class="flex items-start gap-3 p-3 bg-surface-alt rounded-lg"
      >
        <Icon name="folder" size="md" class="text-secondary shrink-0 mt-0.5" />
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2 flex-wrap">
            <span class="text-xs text-tertiary">{{ t(`user-addresses-type-${a.address_type}`) }}</span>
            <span
              v-if="a.is_primary"
              class="px-2 py-0.5 rounded-full text-xs font-medium bg-accent/20 text-accent shrink-0"
            >{{ t('user-addresses-primary') }}</span>
            <span
              v-if="a.source"
              class="px-2 py-0.5 rounded-full text-xs bg-surface-hover text-tertiary shrink-0"
            >{{ t('user-contact-synced-badge') }}</span>
          </div>
          <span class="text-sm text-primary">{{ formatAddress(a) || '—' }}</span>
        </div>
        <div v-if="editable && !a.source" class="flex items-center gap-2 shrink-0">
          <Button v-if="!a.is_primary" variant="secondary" size="sm" @click="setPrimary(a)">
            {{ t('user-addresses-set-primary') }}
          </Button>
          <Button
            variant="ghost-danger"
            size="sm"
            :aria-label="t('common-delete')"
            @click="pendingDelete = a"
          >
            {{ t('common-delete') }}
          </Button>
        </div>
      </div>
    </div>

    <ConfirmModal
      :show="pendingDelete !== null"
      variant="danger"
      :title="t('user-addresses-confirm-title')"
      :message="t('user-addresses-confirm-message')"
      :confirm-label="t('common-delete')"
      @confirm="doDelete"
      @close="pendingDelete = null"
    />
  </SectionCard>
</template>
