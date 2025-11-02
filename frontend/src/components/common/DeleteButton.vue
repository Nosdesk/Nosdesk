<script setup lang="ts">
import { ref } from 'vue';
import { useRouter } from 'vue-router';
import Modal from '@/components/Modal.vue';

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
const router = useRouter();

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
    @click="openConfirmModal"
    class="flex items-center gap-2 px-3 py-1.5 text-sm text-red-400 hover:text-red-300 transition-colors"
  >
    <svg
      xmlns="http://www.w3.org/2000/svg"
      class="h-4 w-4"
      viewBox="0 0 20 20"
      fill="currentColor"
    >
      <path
        fill-rule="evenodd"
        d="M9 2a1 1 0 00-.894.553L7.382 4H4a1 1 0 000 2v10a2 2 0 002 2h8a2 2 0 002-2V6a1 1 0 100-2h-3.382l-.724-1.447A1 1 0 0011 2H9zM7 8a1 1 0 012 0v6a1 1 0 11-2 0V8zm5-1a1 1 0 00-1 1v6a1 1 0 102 0V8a1 1 0 00-1-1z"
        clip-rule="evenodd"
      />
    </svg>
    Delete
  </button>
  
  <!-- Confirmation Modal -->
  <Modal :show="showConfirmModal" title="Confirm Delete" @close="closeConfirmModal">
    <div class="flex flex-col items-center gap-4">
      <div class="mx-auto flex items-center justify-center h-12 w-12 rounded-full bg-red-100 mb-4">
        <svg class="h-6 w-6 text-red-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
        </svg>
      </div>
      
      <h3 class="text-2xl font-medium text-primary mb-2">Delete {{ props.itemName }}</h3>
      <p class="text-base text-secondary mb-6">
        Are you sure you want to delete this {{ props.itemName.toLowerCase() }}? This action cannot be undone.
      </p>

      <div class="flex justify-center gap-4">
        <button
          @click="closeConfirmModal"
          class="px-4 py-2 bg-surface text-primary rounded-lg hover:bg-surface-hover transition-colors"
        >
          Cancel
        </button>
        <button
          @click="confirmDelete"
          class="px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors"
        >
          Delete
        </button>
      </div>
    </div>
  </Modal>
</template>
