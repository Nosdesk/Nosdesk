DROP TRIGGER IF EXISTS tr_audit_tickets ON tickets;
DROP TRIGGER IF EXISTS tr_audit_users ON users;
DROP TRIGGER IF EXISTS tr_audit_groups ON groups;
DROP TRIGGER IF EXISTS tr_audit_ticket_categories ON ticket_categories;
DROP TRIGGER IF EXISTS tr_audit_workflow_states ON workflow_states;
DROP TRIGGER IF EXISTS tr_audit_assignment_rules ON assignment_rules;
DROP TRIGGER IF EXISTS tr_audit_sla_policies ON sla_policies;
DROP TRIGGER IF EXISTS tr_audit_webhooks ON webhooks;
