<!--
The app's toasts, on Reka's Toast. The store (`stores/toast`) is the
queue and the API consumers call; this renders it. Reka owns the parts a
hand-rolled stack gets wrong:
- the timer pauses while the pointer or keyboard focus is on a toast and
  while the window is in the background, and the countdown bar follows it;
- F8 jumps to the viewport (a named region) and Tab walks the toasts'
  controls; Escape on a toast closes it;
- every toast is announced through a live region, assertively for
  errors and notifications (`foreground`), politely for the rest;
- swipe right to dismiss on any pointer, with a click after a swipe
  suppressed;
- `data-state` drives the enter/exit keyframes.

A toast leaves the store only after its exit animation: Reka keeps the
element while `data-state=closed` animates, so the queue holds the entry
until then.
-->
<script setup lang="ts">
import { computed, reactive } from 'vue';
import { useRouter } from 'vue-router';
import { useFluent } from 'fluent-vue';
import {
  ToastAction,
  ToastClose,
  ToastDescription,
  ToastPortal,
  ToastProvider,
  ToastRoot,
  ToastTitle,
  ToastViewport,
} from 'reka-ui';
import { useToastStore, type Toast } from '@nosdesk/core/stores/toast';
import Icon from '@/components/common/Icon.vue';

const fluent = useFluent();
const t = (k: string, args?: Record<string, string | number>) => fluent.$t(k, args);

const toastStore = useToastStore();
const router = useRouter();

const toasts = computed(() => toastStore.visibleToasts);

// Exit animation length; the store entry outlives the close by this much.
const EXIT_MS = 250;
const closing = reactive(new Set<string>());

function onOpenChange(toast: Toast, open: boolean) {
  if (open || closing.has(toast.id)) return;
  closing.add(toast.id);
  setTimeout(() => {
    toastStore.removeToast(toast.id);
    closing.delete(toast.id);
  }, EXIT_MS);
}

// Errors and notifications interrupt; confirmations wait their turn.
const announceType = (type: Toast['type']) =>
  type === 'error' || type === 'notification' ? 'foreground' : 'background';

// Reka reports 0 as "already elapsed"; persistent means no timer.
const timerDuration = (toast: Toast) => (toast.duration > 0 ? toast.duration : Number.POSITIVE_INFINITY);

const getToastClasses = (type: Toast['type']) => {
  // Opaque `bg-surface` base across every type: type is conveyed via the
  // coloured border + ring + icon, not via background tint.
  const base =
    'toast pointer-events-auto w-full max-w-sm rounded-lg bg-surface shadow-lg ring-1 overflow-hidden outline-none focus-visible:ring-2 focus-visible:ring-accent';

  switch (type) {
    case 'success':
      return `${base} ring-status-success/30 border border-status-success/30`;
    case 'warning':
      return `${base} ring-status-warning/30 border border-status-warning/30`;
    case 'error':
      return `${base} ring-status-error/30 border border-status-error/30`;
    case 'notification':
      return `${base} ring-default border border-default hover:border-strong cursor-pointer`;
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
    default:
      return 'text-accent';
  }
};

const getProgressBarClass = (type: Toast['type']) => {
  switch (type) {
    case 'success':
      return 'bg-status-success';
    case 'warning':
      return 'bg-status-warning';
    case 'error':
      return 'bg-status-error';
    default:
      return 'bg-accent';
  }
};

// Notification toasts open the ticket, from a click or Enter on the
// focused toast. Reka cancels the click that ends a swipe, so a
// half-swipe never navigates.
const openNotification = (toast: Toast, event: Event) => {
  if (event.defaultPrevented || !toast.notification) return;
  const { ticketId } = toast.notification;
  if (ticketId) router.push(`/tickets/${ticketId}`);
  toastStore.removeToast(toast.id);
};

// Inline action (e.g. Undo): run the handler; Reka closes the toast.
const invokeAction = (toast: Toast, event: Event) => {
  event.stopPropagation();
  void toast.action?.handler();
};
</script>

<template>
  <ToastProvider :label="t('toast-label')" swipe-direction="right" :swipe-threshold="80">
    <ToastRoot
      v-for="toast in toasts"
      :key="toast.id"
      v-slot="{ remaining, duration }"
      :open="!closing.has(toast.id)"
      :type="announceType(toast.type)"
      :duration="timerDuration(toast)"
      :class="getToastClasses(toast.type)"
      @update:open="onOpenChange(toast, $event)"
      @click="openNotification(toast, $event)"
      @keydown.enter.self="openNotification(toast, $event)"
    >
      <div class="p-4">
        <div class="flex items-start gap-3">
          <!-- Icon -->
          <div class="flex-shrink-0 mt-0.5" :class="getIconClasses(toast.type)" aria-hidden="true">
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
            <ToastTitle as="p" class="text-sm font-medium text-primary break-words">
              {{ toast.title }}
            </ToastTitle>
            <ToastDescription
              v-if="toast.message"
              as="p"
              class="mt-1 text-sm text-secondary break-words"
            >
              {{ toast.message }}
            </ToastDescription>

            <!-- Actor info for notifications -->
            <div v-if="toast.notification?.actorName" class="mt-2 flex items-center gap-2">
              <img
                v-if="toast.notification.actorAvatar"
                :src="toast.notification.actorAvatar"
                alt=""
                class="h-5 w-5 rounded-full object-cover"
              />
              <div
                v-else
                class="h-5 w-5 rounded-full bg-accent/20 flex items-center justify-center"
                aria-hidden="true"
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
            <p v-if="toast.notification" class="mt-2 text-xs text-accent font-medium hover:underline">
              {{ t('toast-notification-view') }}
            </p>
          </div>

          <!-- Inline action button (e.g. Undo). Sits between the
               message and the dismiss button so it reads as part of
               the toast, not chrome. -->
          <ToastAction
            v-if="toast.action"
            as-child
            :alt-text="t('toast-action-alt', { label: toast.action.label, hotkey: 'F8' })"
          >
            <button
              type="button"
              class="flex-shrink-0 inline-flex items-center px-2.5 py-1.5 text-xs font-semibold text-accent rounded-md hover:bg-accent/10 focus:outline-none focus:ring-2 focus:ring-accent transition-colors"
              @click="invokeAction(toast, $event)"
            >
              {{ toast.action.label }}
            </button>
          </ToastAction>

          <!-- Close button -->
          <ToastClose v-if="toast.dismissible" as-child>
            <button
              type="button"
              class="flex-shrink-0 inline-flex rounded-md p-1.5 text-tertiary hover:text-secondary hover:bg-surface-hover focus:outline-none focus:ring-2 focus:ring-accent transition-colors"
              :aria-label="t('common-toast-dismiss')"
              @click.stop
            >
              <Icon name="close" />
            </button>
          </ToastClose>
        </div>
      </div>
      <!-- Countdown bar, driven by Reka's remaining time so it pauses
           with the timer. -->
      <div
        v-if="toast.duration > 0"
        class="h-1 w-full origin-right"
        :class="getProgressBarClass(toast.type)"
        :style="{ transform: `scaleX(${Math.max(0, remaining) / duration})` }"
        aria-hidden="true"
      ></div>
    </ToastRoot>

    <!-- Newest on top: the viewport lists toasts in mount order, so the
         column runs in reverse. Named region; F8 focuses it. -->
    <ToastPortal to="#overlays">
      <ToastViewport
        :label="(hotkey: string) => t('toast-region-label', { hotkey })"
        class="print:hidden pointer-events-none fixed top-0 right-0 flex flex-col-reverse items-end gap-3 px-4 sm:px-6 z-overlay w-full max-w-sm pt-[max(1.5rem,calc(env(safe-area-inset-top)+0.75rem))] list-none m-0 outline-none"
      />
    </ToastPortal>
  </ToastProvider>
</template>

<style>
/* Global: the viewport is portalled out of scoped-style context. */
.toast[data-state='open'] {
  animation: toast-in 300ms ease-out;
}
.toast[data-state='closed'] {
  animation: toast-out 200ms ease-in forwards;
}
.toast[data-swipe='move'] {
  transform: translateX(var(--reka-toast-swipe-move-x));
}
.toast[data-swipe='cancel'] {
  transform: translateX(0);
  transition: transform 200ms ease-out;
}
.toast[data-swipe='end'] {
  animation: toast-swipe-out 150ms ease-out forwards;
}

@keyframes toast-in {
  from {
    opacity: 0;
    transform: translateX(100%);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
@keyframes toast-out {
  from {
    opacity: 1;
    transform: none;
  }
  to {
    opacity: 0;
    transform: translateX(100%);
  }
}
@keyframes toast-swipe-out {
  from {
    transform: translateX(var(--reka-toast-swipe-move-x));
  }
  to {
    transform: translateX(calc(100% + 1rem));
  }
}

@media (prefers-reduced-motion: reduce) {
  .toast[data-state],
  .toast[data-swipe='end'] {
    animation-duration: 1ms;
  }
  .toast[data-swipe='cancel'] {
    transition: none;
  }
}
</style>
