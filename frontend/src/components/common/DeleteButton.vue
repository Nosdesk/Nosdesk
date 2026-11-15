<script setup lang="ts">
import Button from '@/components/common/Button.vue'
import Icon from '@/components/common/Icon.vue'
import { ref } from 'vue';
import { useFluent } from 'fluent-vue';
import Modal from '@/components/Modal.vue';

const fluent = useFluent();
const t = (key: string, args?: Record<string, string | number>) => fluent.$t(key, args);

interface Props {
  fallbackRoute?: string;
  itemName?: string;
}

const props = withDefaults(defineProps<Props>(), {
  fallbackRoute: '/',
  itemName: 'item'
});

const emit = defineEmits<{
  (e: 'delete'): void;
}>();

const showConfirmModal = ref(false);

const openConfirmModal = () => {
  showConfirmModal.value = true;
};

const closeConfirmModal = () => {
  showConfirmModal.value = false;
};

const confirmDelete = () => {
  emit('delete');
  closeConfirmModal();
  
  // The parent component should handle navigation after successful deletion
  // We no longer automatically navigate here to prevent conflicts
};
</script>

<template>
  <button
    type="button"
    @click="openConfirmModal"
    class="flex items-center gap-2 px-3 py-1.5 text-sm text-status-error hover:opacity-80 transition-colors"
  >
    <Icon name="trash" />
    Delete
  </button>

  <!-- Confirmation Modal -->
  <Modal :show="showConfirmModal" :title="t('common-confirm-delete-title')" @close="closeConfirmModal">
    <div class="flex flex-col items-center gap-4">
      <div class="mx-auto flex items-center justify-center h-12 w-12 rounded-full bg-status-error/20 mb-4">
        <Icon name="warning" size="lg" class="text-status-error" />
      </div>

      <h3 class="text-2xl font-medium text-primary mb-2">{{ t('common-delete-item-aria', { name: props.itemName }) }}</h3>
      <p class="text-base text-secondary mb-6">
        Are you sure you want to delete this {{ props.itemName.toLowerCase() }}? This action cannot be undone.
      </p>

      <div class="flex justify-center gap-4">
        <button
          type="button"
          @click="closeConfirmModal"
          class="px-4 py-2 bg-surface text-primary rounded-lg hover:bg-surface-hover transition-colors"
        >
          Cancel
        </button>
        <Button
          @click="confirmDelete"
          variant="danger"
        >
          Delete
        </Button>
      </div>
    </div>
  </Modal>
</template>
