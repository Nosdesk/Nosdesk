<script setup lang="ts">
/**
 * Browseable list of curated starter templates the admin picks
 * from when creating a new canned response. Selecting one fires
 * an event with the chosen starter; the parent navigates into the
 * editor pre-filled with the starter's title + body. Nothing is
 * persisted from here; saving still goes through the normal
 * create handler with the same validation.
 *
 * Lazy-loads the catalog the first time the modal opens so the
 * admin pays the round-trip only if they choose this path.
 */
import { ref, watch } from 'vue';
import { useFluent } from 'fluent-vue';
import AlertMessage from '@/components/common/AlertMessage.vue';
import Button from '@/components/common/Button.vue';
import Modal from '@/components/Modal.vue';
import Skeleton from '@/components/common/Skeleton.vue';
import SkeletonBar from '@/components/common/SkeletonBar.vue';
import cannedResponsesService, {
  type CannedResponseStarter,
} from '@/services/cannedResponsesService';

const { $t } = useFluent();

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{
  close: [];
  select: [starter: CannedResponseStarter];
}>();

const starters = ref<CannedResponseStarter[] | null>(null);
const loadError = ref('');
const isLoading = ref(false);

async function ensureLoaded(): Promise<void> {
  if (starters.value || isLoading.value) return;
  isLoading.value = true;
  loadError.value = '';
  try {
    starters.value = await cannedResponsesService.getStarterCatalog();
  } catch {
    loadError.value = $t('admin-canned-responses-starters-error-load');
  } finally {
    isLoading.value = false;
  }
}

watch(
  () => props.show,
  (open) => {
    if (open) void ensureLoaded();
  },
);

function pick(starter: CannedResponseStarter): void {
  emit('select', starter);
}
</script>

<template>
  <Modal
    :show="show"
    :title="$t('admin-canned-responses-starters-title')"
    size="lg"
    @close="emit('close')"
  >
    <div class="flex flex-col gap-3">
      <p class="text-sm text-secondary">
        {{ $t('admin-canned-responses-starters-description') }}
      </p>

      <AlertMessage v-if="loadError" type="error" :message="loadError" />

      <Skeleton
        v-if="isLoading && !starters"
        :label="$t('admin-canned-responses-starters-loading')"
        class="flex flex-col gap-2"
      >
        <div
          v-for="n in 5"
          :key="n"
          class="border border-default rounded-lg p-3 flex flex-col gap-2"
        >
          <SkeletonBar class="h-4 w-40" />
          <SkeletonBar class="h-3 w-3/4" />
        </div>
      </Skeleton>

      <div v-else-if="starters" class="flex flex-col gap-2">
        <button
          v-for="starter in starters"
          :key="starter.slug"
          type="button"
          class="text-left border border-default rounded-lg p-3 hover:border-accent hover:bg-surface-hover transition-colors flex flex-col gap-1"
          @click="pick(starter)"
        >
          <div class="flex items-center justify-between gap-2">
            <h3 class="font-medium text-primary">{{ starter.title }}</h3>
            <span class="text-xs text-accent">
              {{ $t('admin-canned-responses-starters-use') }} &rarr;
            </span>
          </div>
          <p class="text-xs text-secondary line-clamp-2 whitespace-pre-line">
            {{ starter.body }}
          </p>
        </button>
      </div>

      <div class="flex justify-end pt-2">
        <Button variant="secondary" type="button" @click="emit('close')">
          {{ $t('admin-canned-responses-cancel') }}
        </Button>
      </div>
    </div>
  </Modal>
</template>
