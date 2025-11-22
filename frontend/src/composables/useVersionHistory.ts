import { ref, computed } from 'vue';
import type { Ref } from 'vue';
import versionHistoryService from '@/services/versionHistoryService';
import type { ArticleRevision, ArticleRevisionDetail } from '@/services/versionHistoryService';

export function useVersionHistory(ticketId: Ref<number> | number) {
  const revisions = ref<ArticleRevision[]>([]);
  const isLoading = ref(false);
  const error = ref<Error | null>(null);
  const isRestoring = ref(false);
  const restoreError = ref<Error | null>(null);

  // Convert ticketId to a ref if it's a plain number
  const ticketIdRef = typeof ticketId === 'number' ? ref(ticketId) : ticketId;

  /**
   * Load all revisions for the ticket
   */
  const loadRevisions = async () => {
    isLoading.value = true;
    error.value = null;

    try {
      console.log('[useVersionHistory] Loading revisions for ticket:', ticketIdRef.value);
      revisions.value = await versionHistoryService.getRevisions(ticketIdRef.value);
      console.log('[useVersionHistory] Loaded revisions:', revisions.value);
    } catch (err) {
      error.value = err instanceof Error ? err : new Error('Failed to load revisions');
      console.error('[useVersionHistory] Error loading revisions:', err);
    } finally {
      isLoading.value = false;
    }
  };

  /**
   * Get a specific revision with full content
   * @param revisionNumber - The revision number to fetch
   * @returns The revision detail or null if error
   */
  const getRevisionDetail = async (revisionNumber: number): Promise<ArticleRevisionDetail | null> => {
    try {
      return await versionHistoryService.getRevision(ticketIdRef.value, revisionNumber);
    } catch (err) {
      console.error('Error fetching revision detail:', err);
      return null;
    }
  };

  /**
   * Restore the ticket to a specific revision
   * @param revisionNumber - The revision number to restore
   * @returns True if successful, false otherwise
   */
  const restoreToRevision = async (revisionNumber: number): Promise<boolean> => {
    isRestoring.value = true;
    restoreError.value = null;

    try {
      await versionHistoryService.restoreRevision(ticketIdRef.value, revisionNumber);
      // Reload revisions after restore
      await loadRevisions();
      return true;
    } catch (err) {
      restoreError.value = err instanceof Error ? err : new Error('Failed to restore revision');
      console.error('Error restoring revision:', err);
      return false;
    } finally {
      isRestoring.value = false;
    }
  };

  /**
   * Check if there are any revisions available
   */
  const hasRevisions = computed(() => revisions.value.length > 0);

  /**
   * Get the total number of revisions
   */
  const revisionCount = computed(() => revisions.value.length);

  /**
   * Get the most recent revision (if any)
   */
  const latestRevision = computed(() =>
    revisions.value.length > 0 ? revisions.value[0] : null
  );

  return {
    revisions,
    isLoading,
    error,
    isRestoring,
    restoreError,
    hasRevisions,
    revisionCount,
    latestRevision,
    loadRevisions,
    getRevisionDetail,
    restoreToRevision,
  };
}
