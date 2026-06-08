<template>
  <Modal :show="isOpen" :title="$t('forgot-password-title')" size="sm" @close="close">
    <!-- Success State -->
    <div v-if="emailSent" class="flex flex-col items-center gap-4 text-center">
      <div class="bg-status-success/20 rounded-full p-3">
        <svg class="w-8 h-8 text-status-success" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z"></path>
        </svg>
      </div>
      <div>
        <h3 class="text-lg font-medium text-primary mb-2">{{ $t('forgot-password-success-title') }}</h3>
        <p class="text-sm text-secondary">
          {{ $t('forgot-password-success-body', { email }) }}
        </p>
      </div>
      <div class="bg-accent/10 border border-accent/20 rounded-lg p-4 text-sm text-secondary">
        <p class="mb-2"><strong class="text-accent">{{ $t('forgot-password-success-important') }}</strong></p>
        <ul class="flex flex-col gap-1 text-xs">
          <li>• <span v-html="$t('forgot-password-success-tip-expiry')"></span></li>
          <li>• {{ $t('forgot-password-success-tip-spam') }}</li>
          <li>• {{ $t('forgot-password-success-tip-close') }}</li>
        </ul>
      </div>
      <Button block @click="close">{{ $t('forgot-password-success-done') }}</Button>
    </div>

    <!-- Form State -->
    <form v-else @submit.prevent="handleSubmit" class="flex flex-col gap-4">
      <p class="text-sm text-secondary">
        {{ $t('forgot-password-intro') }}
      </p>

      <AlertMessage v-if="errorMessage" type="error" :message="errorMessage" />

      <FormInput
        id="reset-email"
        v-model="email"
        type="email"
        :label="$t('forgot-password-email-label')"
        :placeholder="$t('forgot-password-email-placeholder')"
        autocomplete="email"
        required
        :disabled="loading"
      />

      <!-- Action Buttons -->
      <div class="flex gap-3 pt-2">
        <Button type="button" variant="secondary" class="flex-1" :disabled="loading" @click="close">
          {{ $t('forgot-password-cancel') }}
        </Button>
        <Button type="submit" class="flex-1" :loading="loading" :disabled="!email">
          {{ loading ? $t('forgot-password-submitting') : $t('forgot-password-submit') }}
        </Button>
      </div>
    </form>
  </Modal>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import authService from '@/services/authService';
import { extractErrorMessage } from '@/utils/errors';
import Modal from '@/components/Modal.vue';
import Button from '@/components/common/Button.vue';
import FormInput from '@/components/common/FormInput.vue';
import AlertMessage from '@/components/common/AlertMessage.vue';

const fluent = useFluent();

const props = defineProps<{
  isOpen: boolean;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const email = ref('');
const loading = ref(false);
const emailSent = ref(false);
const errorMessage = ref('');

// Reset state when modal opens
watch(() => props.isOpen, (newValue) => {
  if (newValue) {
    email.value = '';
    emailSent.value = false;
    errorMessage.value = '';
    loading.value = false;
  }
});

const handleSubmit = async () => {
  errorMessage.value = '';
  loading.value = true;

  try {
    await authService.requestPasswordReset(email.value);
    emailSent.value = true;
  } catch (error) {
    console.error('Password reset request error:', error);
    errorMessage.value = extractErrorMessage(error, fluent.$t('forgot-password-error-default'));
  } finally {
    loading.value = false;
  }
};

const close = () => {
  if (!loading.value) {
    emit('close');
  }
};
</script>
