import { logger } from '@/utils/logger';
import apiClient from '@nosdesk/core/apiClient';

export interface Collection {
  id: number;
  uuid: string;
  name: string;
  slug: string;
  /** Short tagline shown above the rich description editor. */
  description: string | null;
  /** Plain-text projection of the rich description for search. */
  description_text: string | null;
  /**
   * Yjs collaboration room ID for the collection's rich description.
   * Frontend binds the editor to this room; the backend resolves
   * it to documentation_collections.description_yjs.
   */
  description_doc_id: string;
  /**
   * When true, cross-collection wikilinks render as "Restricted
   * page" for viewers without read access (instead of leaking the
   * page title). Opt-in per collection for sensitive content.
   */
  hide_titles_from_non_members: boolean;
  /**
   * When true, pages in this collection that have never been verified
   * show a "needs verification" prompt. Off by default: an unverified
   * page is neutral, not unchecked. Opt-in per collection for
   * compliance content (SOPs, policies).
   */
  require_verification: boolean;
  icon: string | null;
  color: string | null;
  is_system: boolean;
  created_by: string | null;
  created_at: string;
  updated_at: string;
}

export interface VisibleUser {
  uuid: string;
  name: string;
  avatar_url?: string | null;
  avatar_thumb?: string | null;
}

export interface CollectionWithDetails extends Collection {
  visible_to_groups: Array<{ id: number; name: string }>;
  visible_to_users: VisibleUser[];
  is_public: boolean;
  page_count: number;
}

export interface CollectionPage {
  id: number;
  uuid: string;
  title: string;
  slug: string;
  icon: string | null;
  status: string;
  parent_id: number | null;
  display_order: number | null;
  created_at?: string;
  updated_at?: string;
}

export interface CollectionPageTreeNode extends CollectionPage {
  children: CollectionPageTreeNode[];
}

export interface CollectionWithPages extends Collection {
  pages: CollectionPage[];
  visible_to_groups: Array<{ id: number; name: string }>;
  visible_to_users: VisibleUser[];
  is_public: boolean;
  page_count: number;
}

// List visible collections
export const getCollections = async (): Promise<CollectionWithDetails[]> => {
  try {
    const response = await apiClient.get('/documentation/collections');
    return response.data;
  } catch (error) {
    logger.error('Error fetching collections:', error);
    return [];
  }
};

// Get a single collection by ID with pages
export const getCollection = async (id: number): Promise<CollectionWithPages | null> => {
  try {
    const response = await apiClient.get(`/documentation/collections/${id}`);
    return response.data;
  } catch (error) {
    logger.error('Error fetching collection:', error);
    return null;
  }
};

// Get a single collection by slug with pages
export const getCollectionBySlug = async (slug: string): Promise<CollectionWithPages | null> => {
  try {
    const response = await apiClient.get(`/documentation/collections/slug/${slug}`);
    return response.data;
  } catch (error) {
    logger.error('Error fetching collection by slug:', error);
    return null;
  }
};

// Create a new collection
export const createCollection = async (data: {
  name: string;
  slug?: string;
  description?: string;
  icon?: string;
  color?: string;
  visible_to_group_ids?: number[];
}): Promise<Collection | null> => {
  const response = await apiClient.post('/documentation/collections', data);
  return response.data;
};

// Update a collection
export const updateCollection = async (id: number, data: {
  name?: string;
  slug?: string;
  description?: string;
  icon?: string;
  color?: string;
  hide_titles_from_non_members?: boolean;
  require_verification?: boolean;
}): Promise<Collection | null> => {
  try {
    const response = await apiClient.put(`/documentation/collections/${id}`, data);
    return response.data;
  } catch (error) {
    logger.error('Error updating collection:', error);
    return null;
  }
};

// Delete a collection
export const deleteCollection = async (id: number): Promise<boolean> => {
  try {
    await apiClient.delete(`/documentation/collections/${id}`);
    return true;
  } catch (error) {
    logger.error('Error deleting collection:', error);
    return false;
  }
};

// Add a page to a collection
export const addPageToCollection = async (collectionId: number, pageId: number): Promise<boolean> => {
  try {
    await apiClient.post(`/documentation/collections/${collectionId}/pages`, { page_id: pageId });
    return true;
  } catch (error) {
    logger.error('Error adding page to collection:', error);
    return false;
  }
};

// Remove a page from a collection
export const removePageFromCollection = async (collectionId: number, pageId: number): Promise<boolean> => {
  try {
    await apiClient.delete(`/documentation/collections/${collectionId}/pages/${pageId}`);
    return true;
  } catch (error) {
    logger.error('Error removing page from collection:', error);
    return false;
  }
};

// Get pages not in any collection
export const getUncollectedPages = async (): Promise<CollectionPage[]> => {
  try {
    const response = await apiClient.get('/documentation/pages/uncollected');
    return response.data;
  } catch (error) {
    logger.error('Error fetching uncollected pages:', error);
    return [];
  }
};

// Get collections for a specific page
export const getCollectionsForPage = async (pageId: number): Promise<Collection[]> => {
  try {
    const response = await apiClient.get(`/documentation/pages/${pageId}/collections`);
    return response.data;
  } catch (error) {
    logger.error('Error fetching collections for page:', error);
    return [];
  }
};

// Set collections for a page (replaces all memberships)
export const setPageCollections = async (pageId: number, collectionIds: number[]): Promise<Collection[]> => {
  try {
    const response = await apiClient.put(`/documentation/pages/${pageId}/collections`, {
      collection_ids: collectionIds,
    });
    return response.data;
  } catch (error) {
    logger.error('Error setting page collections:', error);
    return [];
  }
};

// Get visibility groups for a collection
export const getCollectionVisibility = async (collectionId: number): Promise<Array<{ id: number; name: string }>> => {
  try {
    const response = await apiClient.get(`/documentation/collections/${collectionId}/visibility`);
    return response.data;
  } catch (error) {
    logger.error('Error fetching collection visibility:', error);
    return [];
  }
};

// Set visibility for a collection (groups and/or users)
export const setCollectionVisibility = async (collectionId: number, groupIds: number[], userUuids: string[] = []): Promise<boolean> => {
  try {
    await apiClient.put(`/documentation/collections/${collectionId}/visibility`, {
      group_ids: groupIds,
      user_uuids: userUuids,
    });
    return true;
  } catch (error) {
    logger.error('Error setting collection visibility:', error);
    return false;
  }
};

// Page override info for collection management
export interface PageOverrideInfo {
  page_id: number;
  page_title: string;
  page_icon: string | null;
  groups: Array<{ id: number; name: string }>;
  users: Array<{ uuid: string; name: string }>;
}

// Get page-level visibility overrides for pages in a collection
export const getPageOverridesInCollection = async (collectionId: number): Promise<PageOverrideInfo[]> => {
  try {
    const response = await apiClient.get(`/documentation/collections/${collectionId}/page-overrides`);
    return response.data;
  } catch (error) {
    logger.error('Error fetching page overrides in collection:', error);
    return [];
  }
};

// Reorder collections
export const reorderCollections = async (collectionOrders: { collection_id: number; display_order: number }[]): Promise<boolean> => {
  try {
    await apiClient.post('/documentation/collections/reorder', {
      collection_orders: collectionOrders,
    });
    return true;
  } catch (error) {
    logger.error('Error reordering collections:', error);
    return false;
  }
};

export default {
  getCollections,
  getCollection,
  getCollectionBySlug,
  createCollection,
  updateCollection,
  deleteCollection,
  addPageToCollection,
  removePageFromCollection,
  getUncollectedPages,
  getCollectionsForPage,
  setPageCollections,
  getCollectionVisibility,
  setCollectionVisibility,
  getPageOverridesInCollection,
  reorderCollections,
};
