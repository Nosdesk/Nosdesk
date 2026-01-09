<script setup lang="ts">
import { computed } from 'vue';
import { useRouter } from 'vue-router';
import { useToastStore, type Toast } from '@/stores/toast';

const toastStore = useToastStore();
const router = useRouter();

const toasts = computed(() => toastStore.visibleToasts);

const getToastClasses = (type: Toast['type']) => {
  const base = 'pointer-events-auto w-full max-w-sm rounded-lg shadow-lg ring-1 overflow-hidden transition-all';

  switch (type) {
    case 'success':
      return `${base} bg-status-success/10 ring-status-success/30 border border-status-success/30`;
    case 'warning':
      return `${base} bg-status-warning/10 ring-status-warning/30 border border-status-warning/30`;
    case 'error':
      return `${base} bg-status-error/10 ring-status-error/30 border border-status-error/30`;
    case 'notification':
      return `${base} bg-surface ring-default border border-default hover:border-strong`;
    default:
      return `${base} bg-accent/10 ring-accent/30 border border-accent/30`;
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
</script>

<template>
  <Teleport to="body">
    <div
      aria-live="assertive"
      class="pointer-events-none fixed inset-0 flex flex-col items-end px-4 py-6 sm:p-6 z-[9999] gap-3"
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
              <div class="flex-shrink-0 mt-0.5">
                <!-- Info icon -->
                <svg
                  v-if="toast.type === 'info'"
                  :class="['h-5 w-5', getIconClasses(toast.type)]"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
                  />
                </svg>

                <!-- Success icon -->
                <svg
                  v-else-if="toast.type === 'success'"
                  :class="['h-5 w-5', getIconClasses(toast.type)]"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
                  />
                </svg>

                <!-- Warning icon -->
                <svg
                  v-else-if="toast.type === 'warning'"
                  :class="['h-5 w-5', getIconClasses(toast.type)]"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z"
                  />
                </svg>

                <!-- Error icon -->
                <svg
                  v-else-if="toast.type === 'error'"
                  :class="['h-5 w-5', getIconClasses(toast.type)]"
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

                <!-- Notification bell icon -->
                <svg
                  v-else-if="toast.type === 'notification'"
                  :class="['h-5 w-5', getIconClasses(toast.type)]"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"
                  />
                </svg>
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

              <!-- Close button -->
              <div v-if="toast.dismissible" class="flex-shrink-0">
                <button
                  @click="dismissToast(toast, $event)"
                  class="inline-flex rounded-md p-1.5 text-tertiary hover:text-secondary hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-accent transition-colors"
                  aria-label="Dismiss"
                >
                  <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M6 18L18 6M6 6l12 12"
                    />
                  </svg>
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
