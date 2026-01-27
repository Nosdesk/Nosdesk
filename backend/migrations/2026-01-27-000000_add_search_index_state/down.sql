-- Revert search_index_state table
DROP INDEX IF EXISTS idx_search_index_state_entity_type;
DROP TABLE IF EXISTS search_index_state;
