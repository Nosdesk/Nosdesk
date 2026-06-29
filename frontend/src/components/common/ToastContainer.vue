<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import { useToastStore, type Toast } from '@nosdesk/core/stores/toast';
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
  // A swipe ends with touchend then a synthetic click; ignore that click so a
  // half-swipe on a notification toast doesn't also navigate (see onTouchEnd).
  if (Date.now() < suppressClickUntil) return;
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

// --- Swipe to dismiss (touch) ---
// Toasts slide in from the right, so a rightward swipe past the threshold
// dismisses; a shorter drag snaps back. Mouse devices never fire these.
const SWIPE_DISMISS_PX = 80;
const swipe = ref<{ id: string; startX: number; dx: number; dragging: boolean } | null>(null);
let suppressClickUntil = 0;

const onTouchStart = (toast: Toast, event: TouchEvent) => {
  swipe.value = { id: toast.id, startX: event.touches[0].clientX, dx: 0, dragging: true };
};

const onTouchMove = (event: TouchEvent) => {
  if (!swipe.value) return;
  swipe.value.dx = Math.max(0, event.touches[0].clientX - swipe.value.startX);
};

const onTouchEnd = (toast: Toast) => {
  const s = swipe.value;
  if (!s || s.id !== toast.id) {
    swipe.value = null;
    return;
  }
  if (s.dx > SWIPE_DISMISS_PX) {
    toastStore.removeToast(toast.id);
    swipe.value = null;
    return;
  }
  // A real drag (not a tap): suppress the click that follows touchend.
  if (s.dx > 8) suppressClickUntil = Date.now() + 300;
  // Snap back with a transition, then clear the swipe state.
  s.dragging = false;
  setTimeout(() => {
    if (swipe.value?.id === toast.id) swipe.value = null;
  }, 200);
};

const swipeStyle = (toast: Toast) => {
  const cursor = toast.notification ? 'pointer' : 'default';
  const s = swipe.value;
  if (s && s.id === toast.id) {
    return {
      cursor,
      transform: `translateX(${s.dx}px)`,
      opacity: String(Math.max(0.25, 1 - s.dx / 240)),
      transition: s.dragging ? 'none' : 'transform 0.2s ease, opacity 0.2s ease',
    };
  }
  return { cursor };
};

const getProgressBarClass = (type: Toast['type']) => {
  switch (type) {
    case 'success':
      return 'bg-status-success';
    case 'warning':
      return 'bg-status-warning';
    case 'error':
      return 'bg-status-error';
    case 'notification':
      return 'bg-accent';
    default:
      return 'bg-accent';
  }
};
</script>

<template>
  <Teleport to="body">
    <div
      aria-live="assertive"
      class="pointer-events-none fixed inset-0 flex flex-col items-end px-4 pb-6 sm:px-6 sm:pb-6 z-overlay gap-3 pt-[max(1.5rem,calc(env(safe-area-inset-top)+0.75rem))]"
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
          @touchstart.passive="onTouchStart(toast, $event)"
          @touchmove.passive="onTouchMove($event)"
          @touchend="onTouchEnd(toast)"
          :style="swipeStyle(toast)"
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
                <p class="text-sm font-medium text-primary break-words">
                  {{ toast.title }}
                </p>
                <p v-if="toast.message" class="mt-1 text-sm text-secondary break-words">
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
          <!-- Auto-dismiss progress bar (only for toasts that time out). -->
          <div
            v-if="toast.duration > 0"
            class="toast-progress h-1 w-full origin-right"
            :class="getProgressBarClass(toast.type)"
            :style="{ animationDuration: toast.duration + 'ms' }"
          ></div>
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

/* Auto-dismiss countdown bar: shrink from full to empty over the toast's
   duration (set inline via animation-duration). */
.toast-progress {
  animation-name: toast-progress;
  animation-timing-function: linear;
  animation-fill-mode: forwards;
}

@keyframes toast-progress {
  from {
    transform: scaleX(1);
  }
  to {
    transform: scaleX(0);
  }
}
</style>
