-- Saved views grow chart-spec columns: viz_type picks the renderer
-- (default 'list' preserves the current behaviour for every existing
-- row), viz_config carries the per-renderer config blob. The CHECK
-- on viz_type enumerates the renderers the frontend SavedViewWidget
-- shell knows how to paint.
--
-- See docs/dashboard-and-analytics-plan.md § 4 for the full schema
-- contract; the chart-source tagged union lives inside viz_config so
-- the column shape stays stable across future renderers.

ALTER TABLE saved_views
    ADD COLUMN viz_type VARCHAR(32) NOT NULL DEFAULT 'list'
        CHECK (viz_type IN (
            'list', 'kpi_tile', 'line', 'horizontal_bar',
            'heatmap', 'leaderboard', 'table'
        )),
    ADD COLUMN viz_config JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Partial index supports the "Your saved views" tab in
-- AddWidgetModal: list workspace-visible saved views whose viz_type
-- is something other than the default list view. archived_at was
-- removed in 2026-05-09-110000_simplify_saved_views so the predicate
-- is just the viz_type filter.
CREATE INDEX saved_views_viz_pickable_idx
    ON saved_views (workspace_id)
    WHERE viz_type <> 'list';
