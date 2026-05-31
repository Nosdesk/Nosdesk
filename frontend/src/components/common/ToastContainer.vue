<script setup lang="ts">
import { computed } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useToastStore, type Toast } from '@/stores/toast';
import Icon from '@/components/common/Icon.vue';

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

const toastStore = useToastStore();
const router = useRouter();

const toasts = computed(() => toastStore.visibleToasts);

const getToastClasses = (type: Toast['type']) => {
  // Opaque `bg-surface` base across every type — the previous
  // `bg-status-*/10` translucent fills inherited whatever was
  // behind them, which read as "frosted glass" rather than a
  // first-class surface. Type is conveyed via the coloured
  // border + ring + icon (kept), not via background tint. The
  // `notification` type already shipped opaque, so this just
  // brings success / warning / error / default into line.
  const base = 'pointer-events-auto w-full max-w-sm rounded-lg bg-surface shadow-lg ring-1 overflow-hidden transition-all';

  switch (type) {
    case 'success':
      return `${base} ring-status-success/30 border border-status-success/30`;
    case 'warning':
      return `${base} ring-status-warning/30 border border-status-warning/30`;
    case 'error':
      return `${base} ring-status-error/30 border border-status-error/30`;
    case 'notification':
      return `${base} ring-default border border-default hover:border-strong`;
    default:
      return `${base} ring-accent/30 border border-accent/30`;
  }
};

const getIconClasses = (type: Toast['type']) => {
  switch (type) {
    case 'success':
      return 'text-status-success';
    case 'warning':
      return 'text-status-warning';
    case 'error':
      return 'text-status-error';
    case 'notification':
      return 'text-accent';
    default:
      return 'text-accent';
  }
};

const handleToastClick = (toast: Toast) => {
  if (toast.notification) {
    const { ticketId } = toast.notification;
    if (ticketId) {
      router.push(`/tickets/${ticketId}`);
    }
    toastStore.removeToast(toast.id);
  }
};

const dismissToast = (toast: Toast, event: Event) => {
  event.stopPropagation();
  toastStore.removeToast(toast.id);
};

// Inline action handler (e.g. Undo). Run the handler then dismiss
// the toast; stopPropagation prevents the parent card click from
// firing for notification toasts.
const invokeAction = async (toast: Toast, event: Event) => {
  event.stopPropagation();
  if (!toast.action) return;
  try {
    await toast.action.handler();
  } finally {
    toastStore.removeToast(toast.id);
  }
};
</script>

<template>
  <Teleport to="body">
    <div
      aria-live="assertive"
      class="pointer-events-none fixed inset-0 flex flex-col items-end px-4 py-6 sm:p-6 z-overlay gap-3"
    >
      <TransitionGroup
        name="toast"
        tag="div"
        class="flex flex-col gap-3 w-full max-w-sm ml-auto"
      >
        <div
          v-for="toast in toasts"
          :key="toast.id"
          :class="getToastClasses(toast.type)"
          @click="toast.notification ? handleToastClick(toast) : undefined"
          :style="{ cursor: toast.notification ? 'pointer' : 'default' }"
          role="alert"
        >
          <div class="p-4">
            <div class="flex items-start gap-3">
              <!-- Icon -->
              <div class="flex-shrink-0 mt-0.5" :class="getIconClasses(toast.type)">
                <Icon v-if="toast.type === 'info'" name="info" size="md" />
                <Icon v-else-if="toast.type === 'success'" name="checkCircle" size="md" />
                <Icon v-else-if="toast.type === 'warning'" name="warning" size="md" />
                <!-- Error icon: X in circle, no registry equivalent -->
                <svg
                  v-else-if="toast.type === 'error'"
                  class="h-5 w-5"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z"
                  />
                </svg>
                <Icon v-else-if="toast.type === 'notification'" name="bell" size="md" />
              </div>

              <!-- Content -->
              <div class="flex-1 pt-0.5 min-w-0">
                <p class="text-sm font-medium text-primary truncate">
                  {{ toast.title }}
                </p>
                <p v-if="toast.message" class="mt-1 text-sm text-secondary line-clamp-2">
                  {{ toast.message }}
                </p>

                <!-- Actor info for notifications -->
                <div
                  v-if="toast.notification?.actorName"
                  class="mt-2 flex items-center gap-2"
                >
                  <img
                    v-if="toast.notification.actorAvatar"
                    :src="toast.notification.actorAvatar"
                    alt=""
                    class="h-5 w-5 rounded-full object-cover"
                  />
                  <div
                    v-else
                    class="h-5 w-5 rounded-full bg-accent/20 flex items-center justify-center"
                  >
                    <span class="text-xs text-accent font-medium">
                      {{ toast.notification.actorName.charAt(0).toUpperCase() }}
                    </span>
                  </div>
                  <span class="text-xs text-tertiary truncate">
                    {{ toast.notification.actorName }}
                  </span>
                </div>

                <!-- View link for notifications -->
                <p
                  v-if="toast.notification"
                  class="mt-2 text-xs text-accent font-medium hover:underline"
                >
                  Click to view
                </p>
              </div>

              <!-- Inline action button (e.g. Undo). Sits between the
                   message and the dismiss × so it reads as part of
                   the toast, not chrome. -->
              <div v-if="toast.action" class="flex-shrink-0">
                <button
                  @click="invokeAction(toast, $event)"
                  class="inline-flex items-center px-2.5 py-1.5 text-xs font-semibold text-accent rounded-md hover:bg-accent/10 focus:outline-none focus:ring-2 focus:ring-accent transition-colors"
                >
                  {{ toast.action.label }}
                </button>
              </div>

              <!-- Close button -->
              <div v-if="toast.dismissible" class="flex-shrink-0">
                <button
                  @click="dismissToast(toast, $event)"
                  class="inline-flex rounded-md p-1.5 text-tertiary hover:text-secondary hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-accent transition-colors"
                  :aria-label="t('common-toast-dismiss')"
                >
                  <Icon name="close" />
                </button>
              </div>
            </div>
          </div>
        </div>
      </TransitionGroup>
    </div>
  </Teleport>
</template>

<style scoped>
.toast-enter-active {
  transition: all 0.3s ease-out;
}

.toast-leave-active {
  transition: all 0.2s ease-in;
}

.toast-enter-from {
  opacity: 0;
  transform: translateX(100%);
}

.toast-leave-to {
  opacity: 0;
  transform: translateX(100%);
}

.toast-move {
  transition: transform 0.3s ease;
}

/* Limit message to 2 lines */
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}
</style>
