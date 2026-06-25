<script setup lang="ts">
/** A user's addresses (vCard ADR): typed, multi-valued, structured. Directory-
 *  synced rows (source set) are read-only. Mirrors UserEmailsCard. */
import { ref, computed, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import SectionCard from '@/components/common/SectionCard.vue';
import ConfirmModal from '@/components/common/ConfirmModal.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import Checkbox from '@/components/common/Checkbox.vue';
import BaseDropdown, { type DropdownOption } from '@/components/common/BaseDropdown.vue';
import Icon from '@/components/common/Icon.vue';
import ContactBadges from '@/components/settings/ContactBadges.vue';
import { useContactList } from '@/composables/useContactList';
import {
  listUserAddresses,
  addUserAddress,
  updateUserAddress,
  deleteUserAddress,
  type UserAddress,
  type UserAddressInput,
} from '@/services/userContactService';

const props = defineProps<{ uuid: string; editable: boolean }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const { items: addresses, showAddForm, saving, pendingDelete, load, add, setPrimary, doDelete } =
  useContactList<UserAddress, UserAddressInput>({
    uuid: () => props.uuid,
    api: {
      list: listUserAddresses,
      add: addUserAddress,
      update: updateUserAddress,
      remove: deleteUserAddress,
    },
    errorKeys: {
      load: 'user-addresses-error-load',
      save: 'user-addresses-error-save',
      delete: 'user-addresses-error-delete',
    },
    toInput: (a) => ({
      address_type: a.address_type,
      street: a.street,
      city: a.city,
      region: a.region,
      postal_code: a.postal_code,
      country: a.country,
      label: a.label,
    }),
  });
watch(() => props.uuid, load, { immediate: true });

const TYPES = ['work', 'home', 'other'];
const typeOptions = computed<DropdownOption[]>(() =>
  TYPES.map((ty) => ({ value: ty, label: t(`user-addresses-type-${ty}`) })),
);

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

function cancel(): void {
  draft.value = blankDraft();
  showAddForm.value = false;
}

const draftEmpty = computed(
  () =>
    !draft.value.street.trim() &&
    !draft.value.city.trim() &&
    !draft.value.region.trim() &&
    !draft.value.postal_code.trim() &&
    !draft.value.country.trim(),
);

async function submit(): Promise<void> {
  if (draftEmpty.value) return;
  const ok = await add({ ...draft.value });
  if (ok) draft.value = blankDraft();
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
      <div class="flex items-center gap-3">
        <BaseDropdown v-model="draft.address_type" :options="typeOptions" class="shrink-0" />
        <Checkbox v-model="draft.is_primary" :label="t('user-addresses-primary')" />
      </div>
      <FormInput v-model="draft.street" :placeholder="t('user-addresses-field-street')" size="sm" />
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
        <FormInput v-model="draft.city" :placeholder="t('user-addresses-field-city')" size="sm" />
        <FormInput v-model="draft.region" :placeholder="t('user-addresses-field-region')" size="sm" />
        <FormInput v-model="draft.postal_code" :placeholder="t('user-addresses-field-postal')" size="sm" />
        <FormInput v-model="draft.country" :placeholder="t('user-addresses-field-country')" size="sm" />
      </div>
      <div class="flex justify-end gap-2">
        <Button size="sm" :loading="saving" :disabled="draftEmpty" @click="submit">
          {{ t('user-addresses-add') }}
        </Button>
        <Button variant="secondary" size="sm" @click="cancel">{{ t('common-cancel') }}</Button>
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
            <ContactBadges
              :primary="a.is_primary"
              :primary-label="t('user-addresses-primary')"
              :synced="!!a.source"
            />
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
