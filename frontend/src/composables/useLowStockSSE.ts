/**
 * useLowStockSSE - global warning-toast listener for the
 * `asset-low-stock` SSE event.
 *
 * Backend emits one of these after a usage decrement that drops
 * a stock-tracked asset's quantity from above to at-or-below its
 * configured `low_stock_threshold`. Edge-detected on the backend
 * so a sequence of below-threshold writes doesn't repeatedly
 * toast.
 *
 * Mount alongside `useNotificationSSE` in App.vue so every
 * authenticated client picks it up regardless of which page
 * they're on.
 */
import { onMounted, onUnmounted } from 'vue';
import { useSSE } from '@/services/sseService';
import { useToastStore } from '@/stores/toast';
import { translate } from '@/i18n';

interface AssetLowStockEventData {
  device_id: number;
  device_name: string;
  quantity: string;
  threshold: string;
  unit: string;
  timestamp: string;
}

export function useLowStockSSE() {
  const { addEventListener, removeEventListener } = useSSE();
  const toastStore = useToastStore();

  const handle = (raw: unknown) => {
    const data = raw as AssetLowStockEventData;
    if (!data || typeof data.device_id !== 'number') return;

    toastStore.warning(
      translate('asset-low-stock-toast-title', { name: data.device_name }),
      translate('asset-low-stock-toast-body', {
        quantity: data.quantity,
        unit: data.unit,
        threshold: data.threshold,
      }),
    );
  };

  onMounted(() => {
    addEventListener('asset-low-stock', handle);
  });

  onUnmounted(() => {
    removeEventListener('asset-low-stock', handle);
  });

  return { handle };
}
