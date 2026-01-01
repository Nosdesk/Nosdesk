import { ref, computed } from 'vue';
import type { Ref } from 'vue';
import versionHistoryService from '@/services/versionHistoryService';
import type { ArticleRevision, ArticleRevisionDetail } from '@/services/versionHistoryService';
import { logger } from '@/utils/logger';

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
      logger.debug('Loading revisions for ticket', { ticketId: ticketIdRef.value });
      revisions.value = await versionHistoryService.getRevisions(ticketIdRef.value);
      logger.debug('Loaded revisions', { count: revisions.value.length });
    } catch (err) {
      error.value = err instanceof Error ? err : new Error('Failed to load revisions');
      logger.error('Error loading revisions', { ticketId: ticketIdRef.value, error: err });
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
      logger.error('Error fetching revision detail', { ticketId: ticketIdRef.value, revisionNumber, error: err });
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
      logger.error('Error restoring revision', { ticketId: ticketIdRef.value, revisionNumber, error: err });
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
