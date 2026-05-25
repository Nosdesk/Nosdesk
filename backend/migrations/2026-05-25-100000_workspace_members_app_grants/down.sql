-- Reverse Item U's grant relaxation: revert to the 3h.6
-- lockdown where nosdesk_app has no access to workspace_members.

REVOKE SELECT, INSERT ON workspace_members FROM nosdesk_app;
