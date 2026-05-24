DROP POLICY IF EXISTS outbound_emails_workspace_isolation ON outbound_emails;
ALTER TABLE outbound_emails NO FORCE ROW LEVEL SECURITY;
ALTER TABLE outbound_emails DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS canned_responses_workspace_isolation ON canned_responses;
ALTER TABLE canned_responses NO FORCE ROW LEVEL SECURITY;
ALTER TABLE canned_responses DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS channel_messages_workspace_isolation ON channel_messages;
ALTER TABLE channel_messages NO FORCE ROW LEVEL SECURITY;
ALTER TABLE channel_messages DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS channel_credentials_workspace_isolation ON channel_credentials;
ALTER TABLE channel_credentials NO FORCE ROW LEVEL SECURITY;
ALTER TABLE channel_credentials DISABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS channels_workspace_isolation ON channels;
ALTER TABLE channels NO FORCE ROW LEVEL SECURITY;
ALTER TABLE channels DISABLE ROW LEVEL SECURITY;
