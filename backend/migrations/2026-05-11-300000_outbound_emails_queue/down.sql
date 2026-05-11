DROP TRIGGER IF EXISTS tr_outbound_emails_notify ON outbound_emails;
DROP FUNCTION IF EXISTS outbound_emails_notify_trigger();
DROP TABLE IF EXISTS outbound_emails;
