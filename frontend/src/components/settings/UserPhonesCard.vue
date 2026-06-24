<script setup lang="ts">
/** A user's phone numbers (vCard TEL): typed, multi-valued, one primary.
 *  Directory-synced rows (source set) are read-only. Mirrors UserEmailsCard. */
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
  listUserPhones,
  addUserPhone,
  updateUserPhone,
  deleteUserPhone,
  type UserPhone,
} from '@/services/userContactService';

const props = defineProps<{ uuid: string; editable: boolean }>();

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);
const toast = useToastStore();

const phones = ref<UserPhone[]>([]);
const showAddForm = ref(false);
const saving = ref(false);
const draft = ref<{ phone: string; phone_type: string; is_primary: boolean }>({
  phone: '',
  phone_type: 'work',
  is_primary: false,
});
const pendingDelete = ref<UserPhone | null>(null);

const TYPES = ['work', 'mobile', 'other'];

async function load(): Promise<void> {
  try {
    phones.value = await listUserPhones(props.uuid);
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-phones-error-load')));
  }
}
watch(() => props.uuid, load, { immediate: true });

function resetDraft(): void {
  draft.value = { phone: '', phone_type: 'work', is_primary: false };
  showAddForm.value = false;
}

async function add(): Promise<void> {
  if (!draft.value.phone.trim()) return;
  saving.value = true;
  try {
    await addUserPhone(props.uuid, {
      phone: draft.value.phone.trim(),
      phone_type: draft.value.phone_type,
      is_primary: draft.value.is_primary,
    });
    resetDraft();
    await load();
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-phones-error-save')));
  } finally {
    saving.value = false;
  }
}

async function setPrimary(p: UserPhone): Promise<void> {
  try {
    await updateUserPhone(props.uuid, p.id, {
      phone: p.phone,
      phone_type: p.phone_type,
      is_primary: true,
      label: p.label,
    });
    await load();
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-phones-error-save')));
  }
}

async function doDelete(): Promise<void> {
  const target = pendingDelete.value;
  pendingDelete.value = null;
  if (!target) return;
  try {
    await deleteUserPhone(props.uuid, target.id);
    await load();
  } catch (err) {
    toast.error(extractErrorMessage(err, t('user-phones-error-delete')));
  }
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
      class="mb-3 p-3 bg-surface-alt rounded-lg border border-subtle flex flex-col sm:flex-row gap-2"
    >
      <select
        v-model="draft.phone_type"
        class="bg-surface border border-default rounded-lg px-2 text-sm text-primary focus:outline-none focus:border-accent"
      >
        <option v-for="ty in TYPES" :key="ty" :value="ty">{{ t(`user-phones-type-${ty}`) }}</option>
      </select>
      <FormInput
        v-model="draft.phone"
        type="tel"
        class="flex-1"
        :placeholder="t('user-phones-add-placeholder')"
        size="sm"
        @keyup.enter="add"
      />
      <label class="flex items-center gap-1.5 text-sm text-secondary whitespace-nowrap">
        <input v-model="draft.is_primary" type="checkbox" /> {{ t('user-phones-primary') }}
      </label>
      <div class="flex gap-2 shrink-0">
        <Button size="sm" :loading="saving" :disabled="!draft.phone.trim()" @click="add">
          {{ t('user-phones-add') }}
        </Button>
        <Button variant="secondary" size="sm" @click="resetDraft">{{ t('common-cancel') }}</Button>
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
            <span
              v-if="p.is_primary"
              class="px-2 py-0.5 rounded-full text-xs font-medium bg-accent/20 text-accent shrink-0"
            >{{ t('user-phones-primary') }}</span>
            <span
              v-if="p.source"
              class="px-2 py-0.5 rounded-full text-xs bg-surface-hover text-tertiary shrink-0"
            >{{ t('user-contact-synced-badge') }}</span>
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
