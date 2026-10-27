<!--
  In-app "Report a problem" modal. User-driven bug report flow.
  Captures the current page, build SHA, viewport, and a snapshot of
  the recent route / API breadcrumb ring, then posts to
  /api/bug-reports. Modal closes within a render frame regardless
  of how long the BE takes; success / failure is communicated via
  toast.
-->
<template>
  <Modal :show="isOpen" :title="$t('bug-report-modal-title')" size="sm" @close="close">
    <form @submit.prevent="handleSubmit" class="flex flex-col gap-4">
      <p class="text-sm text-secondary">
        {{ hosted ? $t('bug-report-modal-attachments-hint-hosted') : $t('bug-report-modal-attachments-hint-self-hosted') }}
      </p>
      <!-- Cloud: account and billing questions need a person, not a report. -->
      <p v-if="hosted" class="flex flex-wrap items-center gap-x-1 text-sm text-secondary">
        {{ $t('bug-report-modal-support-hint') }}
        <SupportAddress />
      </p>

      <FormTextarea
        id="bug-report-description"
        v-model="description"
        :label="$t('bug-report-modal-description-label')"
        :placeholder="$t('bug-report-modal-description-placeholder')"
        :description="$t('bug-report-modal-description-hint')"
        :rows="5"
        :maxlength="4000"
        :disabled="loading"
        required
      />

      <div class="flex gap-3 pt-2">
        <Button type="button" variant="secondary" class="flex-1" :disabled="loading" @click="close">
          {{ $t('bug-report-modal-cancel') }}
        </Button>
        <Button type="submit" class="flex-1" :loading="loading" :disabled="!canSubmit">
          {{ $t('bug-report-modal-submit') }}
        </Button>
      </div>
    </form>
  </Modal>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useFluent } from 'fluent-vue'
import Modal from '@/components/Modal.vue'
import Button from '@/components/common/Button.vue'
import FormTextarea from '@/components/common/FormTextarea.vue'
import SupportAddress from '@/components/SupportAddress.vue'
import { isHostedDeploymentRef } from '@nosdesk/core/services/instanceConfig'
import apiClient from '@nosdesk/core/apiClient'
import { useToastStore } from '@nosdesk/core/stores/toast'
import { getSessionId } from '@/services/diagnostics/session'
import { snapshot as snapshotBreadcrumbs } from '@/services/diagnostics/breadcrumbs'
import { scrubUrl } from '@/services/diagnostics/scrubUrl'

const fluent = useFluent()
const toast = useToastStore()

const props = defineProps<{ isOpen: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const description = ref('')
const loading = ref(false)

// Who reads a report differs by deployment: on Cloud it goes to the Nosdesk
// team; self-hosted, it stays on the server for its operators (the
// `/admin/bug-reports` view, plus NOSDESK_OPS_EMAIL when the operator set one).
const hosted = isHostedDeploymentRef

const canSubmit = computed(() => description.value.trim().length > 0 && !loading.value)

watch(
  () => props.isOpen,
  (open) => {
    if (open) {
      description.value = ''
      loading.value = false
    }
  },
)

function buildPayload() {
  return {
    session_id: getSessionId(),
    build_sha: (import.meta.env.VITE_BUILD_SHA as string | undefined) || 'dev',
    description: description.value.trim(),
    url: scrubUrl(window.location.pathname),
    occurred_at: new Date().toISOString(),
    breadcrumbs: snapshotBreadcrumbs(),
    user_agent: navigator.userAgent,
    viewport: { w: window.innerWidth, h: window.innerHeight },
  }
}

async function handleSubmit() {
  if (!canSubmit.value) return
  loading.value = true
  try {
    await apiClient.post('/bug-reports', buildPayload())
    toast.success(
      fluent.$t('bug-report-success-toast-title'),
      fluent.$t(hosted.value ? 'bug-report-success-toast-body-hosted' : 'bug-report-success-toast-body-self-hosted'),
    )
    emit('close')
  } catch {
    toast.error(
      fluent.$t('bug-report-error-toast-title'),
      fluent.$t('bug-report-error-toast-body'),
    )
  } finally {
    loading.value = false
  }
}

function close() {
  if (!loading.value) emit('close')
}
</script>
