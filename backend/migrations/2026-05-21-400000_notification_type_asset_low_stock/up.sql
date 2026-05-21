-- Promote the SSE-only asset.low_stock alert to a first-class
-- notification type. Recipients are workspace admins and
-- technicians: the people who actually decide on restocks. The
-- triggering user is the one who recorded the consumption that
-- crossed the threshold, so the existing self-skip in
-- NotificationService::notify suppresses their own copy.
--
-- Default channels include email because low-stock signals are
-- the kind of operational event people miss in the bell drop
-- but catch in their inbox. Users can still opt out per-channel
-- via the notification preferences UI.

INSERT INTO notification_types (code, name, description, category, default_channels)
VALUES (
    'asset_low_stock',
    'Asset Low Stock',
    'When a stock-tracked asset''s quantity drops to or below its low-stock threshold',
    'asset',
    '["in_app", "email"]'
);
