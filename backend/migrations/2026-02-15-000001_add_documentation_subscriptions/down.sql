DROP TABLE IF EXISTS documentation_subscriptions;
DELETE FROM notification_types WHERE code = 'doc_page_updated';
