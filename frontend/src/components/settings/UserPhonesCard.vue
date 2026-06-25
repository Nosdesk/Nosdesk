<script setup lang="ts">
/** A user's phone numbers (vCard TEL): typed, multi-valued, one primary.
 *  Directory-synced rows (source set) are read-only. Mirrors UserEmailsCard. */
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
  listUserPhones,
  addUserPhone,
  updateUserPhone,
  deleteUserPhone,
  type UserPhone,
  type UserPhoneInput,
} from '@/services/userContactService';

const props = defineProps<{ uuid: string; editable: boolean }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

const { items: phones, showAddForm, saving, pendingDelete, load, add, setPrimary, doDelete } =
  useContactList<UserPhone, UserPhoneInput>({
    uuid: () => props.uuid,
    api: { list: listUserPhones, add: addUserPhone, update: updateUserPhone, remove: deleteUserPhone },
    errorKeys: {
      load: 'user-phones-error-load',
      save: 'user-phones-error-save',
      delete: 'user-phones-error-delete',
    },
    toInput: (p) => ({ phone: p.phone, phone_type: p.phone_type, label: p.label }),
  });
watch(() => props.uuid, load, { immediate: true });

const TYPES = ['work', 'mobile', 'other'];
const typeOptions = computed<DropdownOption[]>(() =>
  TYPES.map((ty) => ({ value: ty, label: t(`user-phones-type-${ty}`) })),
);

function blankDraft() {
  return { phone: '', phone_type: 'work', is_primary: false };
}
const draft = ref(blankDraft());

function cancel(): void {
  draft.value = blankDraft();
  showAddForm.value = false;
}

async function submit(): Promise<void> {
  if (!draft.value.phone.trim()) return;
  const ok = await add({
    phone: draft.value.phone.trim(),
    phone_type: draft.value.phone_type,
    is_primary: draft.value.is_primary,
  });
  if (ok) draft.value = blankDraft();
}

const confirmMessage = computed(() =>
  pendingDelete.value ? t('user-phones-confirm-message', { phone: pendingDelete.value.phone }) : '',
);
</script>

<template>
  <SectionCard content-padding="p-4">
    <template #title>{{ t('user-phones-title') }}</template>
    <template #headerActions>
      <Button v-if="editable && !showAddForm" size="sm" icon="add" @click="showAddForm = true">
        {{ t('user-phones-add') }}
      </Button>
    </template>

    <div
      v-if="showAddForm && editable"
      class="mb-3 p-3 bg-surface-alt rounded-lg border border-subtle flex flex-col sm:flex-row sm:items-center gap-2"
    >
      <BaseDropdown v-model="draft.phone_type" :options="typeOptions" class="shrink-0" />
      <FormInput
        v-model="draft.phone"
        type="tel"
        class="flex-1"
        :placeholder="t('user-phones-add-placeholder')"
        size="sm"
        @keyup.enter="submit"
      />
      <Checkbox v-model="draft.is_primary" :label="t('user-phones-primary')" />
      <div class="flex gap-2 shrink-0">
        <Button size="sm" :loading="saving" :disabled="!draft.phone.trim()" @click="submit">
          {{ t('user-phones-add') }}
        </Button>
        <Button variant="secondary" size="sm" @click="cancel">{{ t('common-cancel') }}</Button>
      </div>
    </div>

    <p v-if="phones.length === 0" class="text-sm text-tertiary py-2">{{ t('user-phones-empty') }}</p>

    <div v-else class="flex flex-col gap-2">
      <div
        v-for="p in phones"
        :key="p.id"
        class="flex items-center gap-3 p-3 bg-surface-alt rounded-lg"
      >
        <Icon name="phone" size="md" class="text-secondary shrink-0" />
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2 flex-wrap">
            <span class="text-sm font-medium text-primary truncate">{{ p.phone }}</span>
            <ContactBadges
              :primary="p.is_primary"
              :primary-label="t('user-phones-primary')"
              :synced="!!p.source"
            />
          </div>
          <span class="text-xs text-tertiary">{{ t(`user-phones-type-${p.phone_type}`) }}</span>
        </div>
        <div v-if="editable && !p.source" class="flex items-center gap-2 shrink-0">
          <Button v-if="!p.is_primary" variant="secondary" size="sm" @click="setPrimary(p)">
            {{ t('user-phones-set-primary') }}
          </Button>
          <Button
            variant="ghost-danger"
            size="sm"
            :aria-label="t('common-delete')"
            @click="pendingDelete = p"
          >
            {{ t('common-delete') }}
          </Button>
        </div>
      </div>
    </div>

    <ConfirmModal
      :show="pendingDelete !== null"
      variant="danger"
      :title="t('user-phones-confirm-title')"
      :message="confirmMessage"
      :confirm-label="t('common-delete')"
      @confirm="doDelete"
      @close="pendingDelete = null"
    />
  </SectionCard>
</template>
