--
-- PostgreSQL database dump
--


-- Dumped from database version 17.10 (Debian 17.10-1.pgdg12+1)
-- Dumped by pg_dump version 17.10 (Debian 17.10-1.pgdg12+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Data for Name: users; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.users (uuid, name, created_at, updated_at, password_changed_at, pronouns, avatar_url, banner_url, avatar_thumb, microsoft_uuid, mfa_enabled, feature_flag_overrides, deleted_at, mfa_secret, mfa_secret_kek_id, platform_role) VALUES ('01a1230a-afb4-76c2-946b-2607d67dd3e8', 'Fixture Admin', '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:06.643769+00', NULL, NULL, NULL, NULL, NULL, NULL, false, '{}', NULL, NULL, NULL, 'platform_admin');


--
-- Data for Name: active_sessions; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.active_sessions (id, user_uuid, device_name, ip_address, user_agent, location, created_at, last_active, expires_at, is_current, session_id) VALUES (1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'Unknown Asset', '192.168.148.1', 'Python-urllib/3.14', NULL, '2026-10-09 23:41:06.161593+00', '2026-10-09 23:41:06.161593+00', '2026-10-16 23:41:06.160999+00', true, '31b08365-0908-4b90-ac6f-f7e9f4cc997d');


--
-- Data for Name: workspaces; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.workspaces (id, uuid, slug, name, plan, settings, created_at, archived_at, organisation_id, custom_domain, seat_limit) VALUES (1, '2d979007-142d-4acf-9a59-35d3543f7c0e', 'default', 'Workspace', 'self_hosted', '{}', '2026-06-06 05:58:59.333557+00', NULL, NULL, NULL, NULL);


--
-- Data for Name: api_tokens; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: channels; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: ticket_categories; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.ticket_categories (id, uuid, name, description, color, icon, display_order, is_active, created_at, updated_at, created_by, workspace_id) VALUES (2, '0310f156-d429-4b1c-ac1f-e7d6123f75a1', 'Support', 'General help requests', '#3b82f6', 'question', 0, true, '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:05.333158+00', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 1);
INSERT INTO public.ticket_categories (id, uuid, name, description, color, icon, display_order, is_active, created_at, updated_at, created_by, workspace_id) VALUES (3, 'df3107fa-fc9b-4d17-8f20-3f0149c28de0', 'Bug', 'Defect reports', '#ef4444', 'bug', 1, true, '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:05.333158+00', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 1);
INSERT INTO public.ticket_categories (id, uuid, name, description, color, icon, display_order, is_active, created_at, updated_at, created_by, workspace_id) VALUES (4, '3d0827fa-024a-4262-afb8-4f8ab16a9d26', 'Feature request', 'Enhancement ideas', '#8b5cf6', 'lightbulb', 2, true, '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:05.333158+00', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 1);


--
-- Data for Name: workflow_states; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.workflow_states (id, name, category, color, "position", is_default, archived_at, created_at, created_by, workspace_id, pauses_sla) VALUES (1, 'Triage', 'triage', 'slate', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states (id, name, category, color, "position", is_default, archived_at, created_at, created_by, workspace_id, pauses_sla) VALUES (2, 'Backlog', 'backlog', 'gray', 0, true, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states (id, name, category, color, "position", is_default, archived_at, created_at, created_by, workspace_id, pauses_sla) VALUES (4, 'In Review', 'in_review', 'purple', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states (id, name, category, color, "position", is_default, archived_at, created_at, created_by, workspace_id, pauses_sla) VALUES (5, 'Done', 'done', 'green', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states (id, name, category, color, "position", is_default, archived_at, created_at, created_by, workspace_id, pauses_sla) VALUES (6, 'Cancelled', 'cancelled', 'subtle', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, true);
INSERT INTO public.workflow_states (id, name, category, color, "position", is_default, archived_at, created_at, created_by, workspace_id, pauses_sla) VALUES (3, 'In Progress', 'active', 'blue', 0, false, NULL, '2026-06-06 05:58:59.21425+00', NULL, 1, false);
INSERT INTO public.workflow_states (id, name, category, color, "position", is_default, archived_at, created_at, created_by, workspace_id, pauses_sla) VALUES (7, 'Merged', 'merged', 'subtle', 0, false, NULL, '2026-06-06 05:58:59.50906+00', NULL, 1, true);


--
-- Data for Name: tickets; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.tickets (id, title, priority, requester_uuid, assignee_uuid, created_at, updated_at, created_by, closed_at, closed_by, category_id, submitted_via, guest_lookup_token, verification_state, origin_channel_id, workflow_state_id, triage_state, due_date, recurrence_rule, recurrence_template_id, resolution_notes, workspace_id, first_response_at, sla_response_target_at, sla_response_breached_at, sla_resolution_target_at, sla_resolution_breached_at, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, uuid) VALUES (1, 'Printer on level 3 jams', 'medium', '01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, '2026-10-09 23:41:06.177313+00', '2026-10-09 23:41:06.246844+00', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 2, NULL, NULL, NULL, NULL, NULL, 1, '2026-10-09 23:41:06.246844+00', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '01a1230a-b303-7f09-9566-82f111145b63');
INSERT INTO public.tickets (id, title, priority, requester_uuid, assignee_uuid, created_at, updated_at, created_by, closed_at, closed_by, category_id, submitted_via, guest_lookup_token, verification_state, origin_channel_id, workflow_state_id, triage_state, due_date, recurrence_rule, recurrence_template_id, resolution_notes, workspace_id, first_response_at, sla_response_target_at, sla_response_breached_at, sla_resolution_target_at, sla_resolution_breached_at, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, uuid) VALUES (3, 'VPN drops every hour', 'medium', '01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, '2026-10-09 23:41:06.19757+00', '2026-10-09 23:41:06.286003+00', NULL, '2026-10-09 23:41:06.287458+00', NULL, NULL, NULL, NULL, NULL, NULL, 5, NULL, NULL, NULL, NULL, NULL, 1, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '01a1230a-b316-7654-b700-543a9197e3d3');
INSERT INTO public.tickets (id, title, priority, requester_uuid, assignee_uuid, created_at, updated_at, created_by, closed_at, closed_by, category_id, submitted_via, guest_lookup_token, verification_state, origin_channel_id, workflow_state_id, triage_state, due_date, recurrence_rule, recurrence_template_id, resolution_notes, workspace_id, first_response_at, sla_response_target_at, sla_response_breached_at, sla_resolution_target_at, sla_resolution_breached_at, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, uuid) VALUES (4, 'Laptop battery swollen', 'medium', '01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, '2026-10-09 23:41:06.202522+00', '2026-10-09 23:41:06.302058+00', NULL, '2026-10-09 23:41:06.30222+00', NULL, NULL, NULL, NULL, NULL, NULL, 5, NULL, NULL, NULL, NULL, NULL, 1, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, '01a1230a-b31b-70ff-b517-f5425cbd8d1e');
INSERT INTO public.tickets (id, title, priority, requester_uuid, assignee_uuid, created_at, updated_at, created_by, closed_at, closed_by, category_id, submitted_via, guest_lookup_token, verification_state, origin_channel_id, workflow_state_id, triage_state, due_date, recurrence_rule, recurrence_template_id, resolution_notes, workspace_id, first_response_at, sla_response_target_at, sla_response_breached_at, sla_resolution_target_at, sla_resolution_breached_at, merged_into_ticket_id, merged_at, merged_by_user_uuid, merge_reason, uuid) VALUES (2, 'Printer level 3 jams again', 'medium', '01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, '2026-10-09 23:41:06.189926+00', '2026-10-09 23:41:06.315751+00', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, 7, NULL, NULL, NULL, NULL, NULL, 1, NULL, NULL, NULL, NULL, NULL, 1, '2026-10-09 23:41:06.315751+00', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'Duplicate', '01a1230a-b30f-71ed-b15b-780e08e8f7bd');


--
-- Data for Name: article_contents; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: article_content_revisions; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: assets; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: asset_audits; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: groups; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: asset_groups; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: asset_kinds; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (13, 'generic', 'Generic asset', 'A workspace-neutral asset. Use for anything that does not fit a more specific kind.', 'asset', '{"type": "object", "properties": {}}', 5, true, '2026-06-06 05:58:59.307074+00', '2026-06-06 05:58:59.307074+00', NULL, 'generic', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (8, 'license', 'License', 'Software license with optional seat tracking.', 'license', '{"type": "object", "properties": {}}', 80, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'logical', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (9, 'vehicle', 'Vehicle', 'Car, van, truck, trailer.', 'vehicle', '{"type": "object", "properties": {}}', 90, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'physical', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (10, 'equipment', 'Equipment', 'Tools, machinery, instruments.', 'equipment', '{"type": "object", "properties": {}}', 100, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'physical', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (11, 'consumable', 'Consumable', 'Items consumed during work (uses quantity + unit).', 'consumable', '{"type": "object", "properties": {}}', 110, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'bulk', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (12, 'material', 'Material', 'Bulk material tracked by quantity (pipe lengths, cable rolls).', 'material', '{"type": "object", "properties": {}}', 120, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'bulk', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (1, 'device', 'Device', 'Generic IT device. Default kind for assets created via the legacy /devices path.', 'device', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 10, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (2, 'laptop', 'Laptop', 'Portable computer assigned to a user.', 'laptop', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 20, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (3, 'desktop', 'Desktop', 'Workstation computer at a fixed location.', 'desktop', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 30, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (4, 'server', 'Server', 'Server hardware in a data centre or office.', 'server', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 40, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (5, 'phone', 'Phone', 'Mobile phone or VoIP handset.', 'phone', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 50, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (6, 'monitor', 'Monitor', 'External display.', 'monitor', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 60, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);
INSERT INTO public.asset_kinds (id, slug, label, description, icon, attribute_schema, sort_order, is_builtin, created_at, updated_at, created_by, category, workspace_id) VALUES (7, 'network_device', 'Network device', 'Switch, router, access point, firewall.', 'network', '{"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}', 70, true, '2026-06-06 05:58:59.299189+00', '2026-06-06 05:58:59.299189+00', NULL, 'it', 1);


--
-- Data for Name: asset_lifecycle_events; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: asset_media; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: asset_usage_log; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: assignment_rules; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: assignment_log; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: assignment_rule_state; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: comments; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.comments (id, content, ticket_id, user_uuid, created_at, updated_at, is_edited, edit_count, channel_metadata, is_internal, deleted_at, content_format, body_text, body_html, new_content, quoted_content, raw_source_uri, workspace_id, render_kind) VALUES (1, '<p>Logs attached</p>', 1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:06.246844+00', '2026-10-09 23:41:06.246844+00', false, 0, NULL, false, NULL, 'html', NULL, NULL, NULL, NULL, NULL, 1, 'simple');
INSERT INTO public.comments (id, content, ticket_id, user_uuid, created_at, updated_at, is_edited, edit_count, channel_metadata, is_internal, deleted_at, content_format, body_text, body_html, new_content, quoted_content, raw_source_uri, workspace_id, render_kind) VALUES (2, 'Merged 1 ticket(s) into this one:
- #2: "Printer level 3 jams again"
Reason: Duplicate', 1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:06.315751+00', '2026-10-09 23:41:06.315751+00', false, 0, '{"kind": "merge_marker", "reason": "Duplicate", "sources": [{"id": 2, "title": "Printer level 3 jams again", "opened_at": "2026-10-09T23:41:06.189926", "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8"}], "source_ticket_ids": [2], "merged_by_user_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "merged_into_ticket_id": 1}', false, NULL, 'html', 'Merged 1 ticket(s) into this one:
- #2: "Printer level 3 jams again"
Reason: Duplicate', '<p>Merged 1 ticket(s) into this one:<br>- #2: "Printer level 3 jams again"<br>Reason: Duplicate</p>', NULL, NULL, NULL, 1, NULL);


--
-- Data for Name: attachments; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.attachments (id, url, name, file_size, mime_type, checksum, comment_id, uploaded_by, created_at, transcription, workspace_id) VALUES (1, '/uploads/tickets/1/01a1230a-b32f-75e3-8494-6ba9d6eb2497_note.txt', 'note.txt', 19, 'application/octet-stream', 'fa7b57ddeea09e347209cca4c51ebb77b9eca97117e1d4fea3126802dee7cc30', 1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:06.224489+00', NULL, 1);
INSERT INTO public.attachments (id, url, name, file_size, mime_type, checksum, comment_id, uploaded_by, created_at, transcription, workspace_id) VALUES (2, '/uploads/tickets/1/01a1230a-b334-7d41-a1df-53c3b6c42fca_pixel.png', 'pixel.png', 70, 'image/png', 'c414cd0e204de974f73753c7e28d7638e7b3691bb8b1a2bab6b25bb7fed7ce77', 1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:06.228565+00', NULL, 1);


--
-- Data for Name: audit_log_2026_05; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log



--
-- Data for Name: audit_log_2026_06; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log

INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (1, 'site_settings', '1', 'U', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.018294+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.236554+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {"projects_v2": true}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{updated_at,feature_flags}', NULL, NULL, '2026-06-06 05:58:59.237271+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (2, 'site_settings', '1', 'U', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.236554+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {"projects_v2": true}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{"id": 1, "app_name": "Nosdesk", "logo_url": null, "created_at": "2026-06-06T05:58:59.018294+00:00", "updated_at": "2026-06-06T05:58:59.238807+00:00", "updated_by": null, "favicon_url": null, "feature_flags": {}, "primary_color": null, "logo_light_url": null, "guest_tickets_enabled": false, "guest_help_page_enabled": false, "guest_kb_search_enabled": false, "channel_auto_ack_enabled": true, "channel_auto_ack_template": null, "guest_public_docs_enabled": false, "guest_ticket_intro_message": null, "guest_ticket_lookup_enabled": false, "guest_ticket_default_priority": null, "guest_ticket_email_verification": true, "guest_ticket_attachments_enabled": true, "guest_ticket_rate_limit_per_hour": 5}', '{updated_at,feature_flags}', NULL, NULL, '2026-06-06 05:58:59.239076+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (3, 'asset_kinds', '13', 'I', NULL, '{"id": 13, "icon": "asset", "slug": "generic", "label": "Generic asset", "created_at": "2026-06-06T05:58:59.307074+00:00", "created_by": null, "is_builtin": true, "sort_order": 5, "updated_at": "2026-06-06T05:58:59.307074+00:00", "description": "A workspace-neutral asset. Use for anything that does not fit a more specific kind.", "attribute_schema": {"type": "object", "properties": {}}}', NULL, NULL, NULL, '2026-06-06 05:58:59.307359+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (4, 'asset_kinds', '1', 'U', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.308976+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (5, 'asset_kinds', '2', 'U', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309055+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (6, 'asset_kinds', '3', 'U', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309129+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (7, 'asset_kinds', '4', 'U', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309199+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (8, 'asset_kinds', '5', 'U', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309268+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (9, 'asset_kinds', '6', 'U', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.30934+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (10, 'asset_kinds', '7', 'U', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.30955+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (11, 'asset_kinds', '8', 'U', '{"id": 8, "icon": "license", "slug": "license", "label": "License", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 80, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Software license with optional seat tracking.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 8, "icon": "license", "slug": "license", "label": "License", "category": "logical", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 80, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Software license with optional seat tracking.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309638+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (12, 'asset_kinds', '9', 'U', '{"id": 9, "icon": "vehicle", "slug": "vehicle", "label": "Vehicle", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 90, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Car, van, truck, trailer.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 9, "icon": "vehicle", "slug": "vehicle", "label": "Vehicle", "category": "physical", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 90, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Car, van, truck, trailer.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.30973+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (13, 'asset_kinds', '10', 'U', '{"id": 10, "icon": "equipment", "slug": "equipment", "label": "Equipment", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 100, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Tools, machinery, instruments.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 10, "icon": "equipment", "slug": "equipment", "label": "Equipment", "category": "physical", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 100, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Tools, machinery, instruments.", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309791+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (14, 'asset_kinds', '11', 'U', '{"id": 11, "icon": "consumable", "slug": "consumable", "label": "Consumable", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 110, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Items consumed during work (uses quantity + unit).", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 11, "icon": "consumable", "slug": "consumable", "label": "Consumable", "category": "bulk", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 110, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Items consumed during work (uses quantity + unit).", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309875+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (15, 'asset_kinds', '12', 'U', '{"id": 12, "icon": "material", "slug": "material", "label": "Material", "category": "generic", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 120, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Bulk material tracked by quantity (pipe lengths, cable rolls).", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 12, "icon": "material", "slug": "material", "label": "Material", "category": "bulk", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 120, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Bulk material tracked by quantity (pipe lengths, cable rolls).", "attribute_schema": {"type": "object", "properties": {}}}', '{category}', NULL, NULL, '2026-06-06 05:58:59.309935+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (16, 'asset_kinds', '1', 'U', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 1, "icon": "device", "slug": "device", "label": "Device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 10, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Generic IT device. Default kind for assets created via the legacy /devices path.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.31314+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (17, 'asset_kinds', '2', 'U', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 2, "icon": "laptop", "slug": "laptop", "label": "Laptop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 20, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Portable computer assigned to a user.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313263+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (18, 'asset_kinds', '3', 'U', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 3, "icon": "desktop", "slug": "desktop", "label": "Desktop", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 30, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Workstation computer at a fixed location.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313395+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (19, 'asset_kinds', '4', 'U', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 4, "icon": "server", "slug": "server", "label": "Server", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 40, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Server hardware in a data centre or office.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313504+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (20, 'asset_kinds', '5', 'U', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 5, "icon": "phone", "slug": "phone", "label": "Phone", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 50, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Mobile phone or VoIP handset.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313595+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (21, 'asset_kinds', '6', 'U', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 6, "icon": "monitor", "slug": "monitor", "label": "Monitor", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 60, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "External display.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313673+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (22, 'asset_kinds', '7', 'U', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {}}}', '{"id": 7, "icon": "network", "slug": "network_device", "label": "Network device", "category": "it", "created_at": "2026-06-06T05:58:59.299189+00:00", "created_by": null, "is_builtin": true, "sort_order": 70, "updated_at": "2026-06-06T05:58:59.299189+00:00", "description": "Switch, router, access point, firewall.", "attribute_schema": {"type": "object", "properties": {"hostname": {"type": "string", "title": "Hostname"}, "is_managed": {"type": "boolean", "title": "Managed"}, "os_version": {"type": "string", "title": "OS version"}, "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"}, "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"}, "entra_device_id": {"type": "string", "title": "Entra device ID"}, "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"}, "compliance_state": {"type": "string", "title": "Compliance state"}, "intune_device_id": {"type": "string", "title": "Intune device ID"}, "operating_system": {"type": "string", "title": "Operating system"}, "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"}, "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"}, "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}}}}', '{attribute_schema}', NULL, NULL, '2026-06-06 05:58:59.313781+00', 1);


--
-- Data for Name: audit_log_2026_07; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log



--
-- Data for Name: audit_log_2026_08; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log



--
-- Data for Name: audit_log_2026_10; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log

INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (25, 'plugin_local_signing_key', '1', 'I', NULL, '{"id": 1, "pubkey": "5GY1jilI+L6cDrzaymzy2K7Mh+/42HrQeKHRq3ibU2I=", "created_at": "2026-10-09T23:40:45.535254+00:00", "fingerprint": "3c889810e10b0e3b", "encrypted_sk": "\\x01010001f86efc1995853a26e2432c022f95cd0b4b4f7172034b9353a945bc30c273b8e290c5f2884b81b31136b3bd185ef5d638754f7420eca31053bce4586cbbba42285b296418c906ffdef211d161bc407e451cac5a300564e187afc9c3fb0e51f2e47282451bc0ddb343086156ad6c5ed0", "encrypted_sk_kek_id": 1}', NULL, NULL, NULL, '2026-10-09 23:40:45.542328+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (26, 'plugin_registry_state', '1', 'U', '{"id": 1, "updated_at": "2026-06-06T05:58:59.183222+00:00", "index_version": 0, "last_fetched_at": null, "last_fetch_error": null, "publishers_version": 0}', '{"id": 1, "updated_at": "2026-10-09T23:40:46.281698+00:00", "index_version": 0, "last_fetched_at": null, "last_fetch_error": "registry signature verification failed", "publishers_version": 0}', '{updated_at,last_fetch_error}', NULL, NULL, '2026-10-09 23:40:46.282425+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (27, 'users', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'I', NULL, '{"uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "pronouns": null, "avatar_url": null, "banner_url": null, "created_at": "2026-10-09T23:41:05.333158+00:00", "deleted_at": null, "updated_at": "2026-10-09T23:41:05.333158+00:00", "mfa_enabled": false, "avatar_thumb": null, "name_changed": true, "platform_role": "platform_admin", "microsoft_uuid": null, "mfa_secret_kek_id": null, "mfa_secret_changed": false, "password_changed_at": null, "feature_flag_overrides": {}, "mfa_backup_codes_changed": false}', NULL, NULL, NULL, '2026-10-09 23:41:05.334983+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (28, 'workspace_members', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'I', NULL, '{"role": "admin", "user_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "invited_at": "2026-10-09T23:41:05.333158+00:00", "accepted_at": "2026-10-09T23:41:05.333158+00:00", "workspace_id": 1}', NULL, NULL, NULL, '2026-10-09 23:41:05.336133+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (29, 'ticket_categories', '2', 'I', NULL, '{"id": 2, "icon": "question", "name": "Support", "uuid": "0310f156-d429-4b1c-ac1f-e7d6123f75a1", "color": "#3b82f6", "is_active": true, "created_at": "2026-10-09T23:41:05.333158+00:00", "created_by": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "updated_at": "2026-10-09T23:41:05.333158+00:00", "description": "General help requests", "workspace_id": 1, "display_order": 0}', NULL, NULL, NULL, '2026-10-09 23:41:05.342061+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (30, 'ticket_categories', '3', 'I', NULL, '{"id": 3, "icon": "bug", "name": "Bug", "uuid": "df3107fa-fc9b-4d17-8f20-3f0149c28de0", "color": "#ef4444", "is_active": true, "created_at": "2026-10-09T23:41:05.333158+00:00", "created_by": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "updated_at": "2026-10-09T23:41:05.333158+00:00", "description": "Defect reports", "workspace_id": 1, "display_order": 1}', NULL, NULL, NULL, '2026-10-09 23:41:05.342141+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (31, 'ticket_categories', '4', 'I', NULL, '{"id": 4, "icon": "lightbulb", "name": "Feature request", "uuid": "3d0827fa-024a-4262-afb8-4f8ab16a9d26", "color": "#8b5cf6", "is_active": true, "created_at": "2026-10-09T23:41:05.333158+00:00", "created_by": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "updated_at": "2026-10-09T23:41:05.333158+00:00", "description": "Enhancement ideas", "workspace_id": 1, "display_order": 2}', NULL, NULL, NULL, '2026-10-09 23:41:05.342212+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (32, 'users', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'U', '{"uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "pronouns": null, "avatar_url": null, "banner_url": null, "created_at": "2026-10-09T23:41:05.333158+00:00", "deleted_at": null, "updated_at": "2026-10-09T23:41:05.333158+00:00", "mfa_enabled": false, "avatar_thumb": null, "platform_role": "platform_admin", "microsoft_uuid": null, "mfa_secret_kek_id": null, "password_changed_at": null, "feature_flag_overrides": {}}', '{"uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "pronouns": null, "avatar_url": null, "banner_url": null, "created_at": "2026-10-09T23:41:05.333158+00:00", "deleted_at": null, "updated_at": "2026-10-09T23:41:06.154167+00:00", "mfa_enabled": true, "avatar_thumb": null, "name_changed": false, "platform_role": "platform_admin", "microsoft_uuid": null, "mfa_secret_kek_id": 1, "mfa_secret_changed": true, "password_changed_at": null, "feature_flag_overrides": {}, "mfa_backup_codes_changed": false}', '{mfa_secret,updated_at,mfa_enabled,mfa_secret_kek_id}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, '2026-10-09 23:41:06.157487+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (33, 'tickets', '1', 'I', NULL, '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.177313+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.177313+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', NULL, '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'fb7954f3-fe1a-49c0-9109-552bd6ce988f', '2026-10-09 23:41:06.17963+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (34, 'tickets', '2', 'I', NULL, '{"id": 2, "uuid": "01a1230a-b30f-71ed-b15b-780e08e8f7bd", "title": "Printer level 3 jams again", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.189926+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.189926+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', NULL, '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'f95515d7-8ce3-4574-93ca-20c03e1c6216', '2026-10-09 23:41:06.191875+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (35, 'tickets', '3', 'I', NULL, '{"id": 3, "uuid": "01a1230a-b316-7654-b700-543a9197e3d3", "title": "VPN drops every hour", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.19757+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.19757+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', NULL, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '46173b4b-6295-4556-9ebd-ffd07ff90f8e', '2026-10-09 23:41:06.198048+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (36, 'tickets', '4', 'I', NULL, '{"id": 4, "uuid": "01a1230a-b31b-70ff-b517-f5425cbd8d1e", "title": "Laptop battery swollen", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.202522+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.202522+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', NULL, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '08539c5e-439e-4bde-966d-8b57b8f6b788', '2026-10-09 23:41:06.202952+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (37, 'tickets', '1', 'U', '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.177313+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.177313+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.177313+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.246844+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{updated_at}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'b53f21c2-28cd-4c85-b6e5-704814493877', '2026-10-09 23:41:06.248934+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (38, 'tickets', '1', 'U', '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.177313+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.246844+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.177313+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.246844+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": "2026-10-09T23:41:06.246844+00:00", "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{first_response_at}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'b53f21c2-28cd-4c85-b6e5-704814493877', '2026-10-09 23:41:06.250501+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (39, 'tickets', '1', 'U', '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.177313+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.246844+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": "2026-10-09T23:41:06.246844+00:00", "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.177313+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.246844+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": "2026-10-09T23:41:06.246844+00:00", "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'b53f21c2-28cd-4c85-b6e5-704814493877', '2026-10-09 23:41:06.254434+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (40, 'tickets', '3', 'U', '{"id": 3, "uuid": "01a1230a-b316-7654-b700-543a9197e3d3", "title": "VPN drops every hour", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.19757+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.19757+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 3, "uuid": "01a1230a-b316-7654-b700-543a9197e3d3", "title": "VPN drops every hour", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.287458+00:00", "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.19757+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.286003+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 5, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{closed_at,updated_at,workflow_state_id}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'b7858158-e0af-4da3-add0-5907e193089f', '2026-10-09 23:41:06.289209+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (41, 'tickets', '3', 'U', '{"id": 3, "uuid": "01a1230a-b316-7654-b700-543a9197e3d3", "title": "VPN drops every hour", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.287458+00:00", "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.19757+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.286003+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 5, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 3, "uuid": "01a1230a-b316-7654-b700-543a9197e3d3", "title": "VPN drops every hour", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.287458+00:00", "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.19757+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.286003+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 5, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'b7858158-e0af-4da3-add0-5907e193089f', '2026-10-09 23:41:06.289903+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (45, 'users', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'U', '{"uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "pronouns": null, "avatar_url": null, "banner_url": null, "created_at": "2026-10-09T23:41:05.333158+00:00", "deleted_at": null, "updated_at": "2026-10-09T23:41:06.154167+00:00", "mfa_enabled": true, "avatar_thumb": null, "platform_role": "platform_admin", "microsoft_uuid": null, "mfa_secret_kek_id": 1, "password_changed_at": null, "feature_flag_overrides": {}}', '{"uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "pronouns": null, "avatar_url": null, "banner_url": null, "created_at": "2026-10-09T23:41:05.333158+00:00", "deleted_at": null, "updated_at": "2026-10-09T23:41:06.643769+00:00", "mfa_enabled": false, "avatar_thumb": null, "name_changed": false, "platform_role": "platform_admin", "microsoft_uuid": null, "mfa_secret_kek_id": null, "mfa_secret_changed": true, "password_changed_at": null, "feature_flag_overrides": {}, "mfa_backup_codes_changed": false}', '{mfa_secret,updated_at,mfa_enabled,mfa_secret_kek_id}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, '2026-10-09 23:41:06.645153+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (42, 'tickets', '4', 'U', '{"id": 4, "uuid": "01a1230a-b31b-70ff-b517-f5425cbd8d1e", "title": "Laptop battery swollen", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.202522+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.202522+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 4, "uuid": "01a1230a-b31b-70ff-b517-f5425cbd8d1e", "title": "Laptop battery swollen", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.30222+00:00", "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.202522+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.302058+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 5, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{closed_at,updated_at,workflow_state_id}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', '7f0e59a3-2c04-4a55-aaa0-f93c14d4e9b9', '2026-10-09 23:41:06.302909+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (43, 'tickets', '4', 'U', '{"id": 4, "uuid": "01a1230a-b31b-70ff-b517-f5425cbd8d1e", "title": "Laptop battery swollen", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.30222+00:00", "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.202522+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.302058+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 5, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 4, "uuid": "01a1230a-b31b-70ff-b517-f5425cbd8d1e", "title": "Laptop battery swollen", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.30222+00:00", "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.202522+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.302058+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 5, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', '7f0e59a3-2c04-4a55-aaa0-f93c14d4e9b9', '2026-10-09 23:41:06.30375+00', 1);
INSERT INTO public.audit_log (id, table_name, pk_text, op, before_jsonb, after_jsonb, changed_cols, actor_uuid, correlation_id, occurred_at, workspace_id) VALUES (44, 'tickets', '2', 'U', '{"id": 2, "uuid": "01a1230a-b30f-71ed-b15b-780e08e8f7bd", "title": "Printer level 3 jams again", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": null, "created_at": "2026-10-09T23:41:06.189926+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.189926+00:00", "category_id": null, "merge_reason": null, "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 2, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": null, "merged_into_ticket_id": null, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{"id": 2, "uuid": "01a1230a-b30f-71ed-b15b-780e08e8f7bd", "title": "Printer level 3 jams again", "due_date": null, "priority": "medium", "closed_at": null, "closed_by": null, "merged_at": "2026-10-09T23:41:06.315751+00:00", "created_at": "2026-10-09T23:41:06.189926+00:00", "created_by": null, "updated_at": "2026-10-09T23:41:06.315751+00:00", "category_id": null, "merge_reason": "Duplicate", "triage_state": null, "workspace_id": 1, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "recurrence_rule": null, "resolution_notes": null, "first_response_at": null, "origin_channel_id": null, "workflow_state_id": 7, "guest_lookup_token": null, "verification_state": null, "merged_by_user_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "merged_into_ticket_id": 1, "recurrence_template_id": null, "sla_response_target_at": null, "sla_resolution_target_at": null, "sla_response_breached_at": null, "sla_resolution_breached_at": null}', '{merged_at,updated_at,merge_reason,workflow_state_id,merged_by_user_uuid,merged_into_ticket_id}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', '6c5712b8-5be8-47c5-89ee-b5e5bfd2573e', '2026-10-09 23:41:06.317851+00', 1);


--
-- Data for Name: audit_log_2026_11; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log



--
-- Data for Name: audit_log_2026_12; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log



--
-- Data for Name: audit_log_default; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.audit_log



--
-- Data for Name: backup_jobs; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.backup_jobs (id, job_type, status, include_sensitive, file_path, file_size, error_message, created_by, created_at, completed_at, workspace_id) VALUES ('01a1230a-b4dc-7aea-a9e1-fd3f6fb168a5', 'export', 'completed', false, '/app/uploads/backups/backup-2026-10-09-234106-654999.zip', 31873, NULL, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:06.651379+00', '2026-10-09 23:41:06.694368+00', 1);
INSERT INTO public.backup_jobs (id, job_type, status, include_sensitive, file_path, file_size, error_message, created_by, created_at, completed_at, workspace_id) VALUES ('01a1230a-b961-7f99-8119-4205b02da2b5', 'export', 'processing', true, NULL, NULL, NULL, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:07.807907+00', NULL, 1);


--
-- Data for Name: bug_reports; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: canned_responses; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: canned_response_insertions; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: category_group_visibility; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: channel_credentials; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: channel_messages; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: csp_reports; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: projects; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: cycles; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: cycle_tickets; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_collections; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.documentation_collections (id, uuid, name, slug, description, icon, color, is_system, created_by, created_at, updated_at, display_order, description_yjs, description_state_vector, description_text, hide_titles_from_non_members, workspace_id, fence_token, require_verification) VALUES (2, 'd130d0e1-8c8a-4664-86e2-aeca506838ac', 'Getting Started', 'getting-started', 'Introduction and onboarding documentation', '🚀', NULL, true, NULL, '2026-06-06 05:58:59.118175+00', '2026-06-06 05:58:59.118175+00', 0, NULL, NULL, NULL, false, 1, NULL, false);
INSERT INTO public.documentation_collections (id, uuid, name, slug, description, icon, color, is_system, created_by, created_at, updated_at, display_order, description_yjs, description_state_vector, description_text, hide_titles_from_non_members, workspace_id, fence_token, require_verification) VALUES (1, '9744d4a0-4ad0-4940-8c9c-f759a0e7902a', 'Tickets', 'tickets', 'Documentation pages created from ticket notes', '🎫', NULL, true, NULL, '2026-06-06 05:58:59.118175+00', '2026-06-06 05:58:59.118175+00', 1, NULL, NULL, NULL, false, 1, NULL, false);


--
-- Data for Name: documentation_pages; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.documentation_pages (id, uuid, title, slug, icon, cover_image, status, created_at, updated_at, created_by, last_edited_by, parent_id, display_order, is_public, is_template, archived_at, yjs_state_vector, yjs_document, yjs_client_id, has_unsaved_changes, deleted_at, verified_by, verified_at, verify_interval_days, workspace_id, fence_token) VALUES (1, '01a1230a-b398-7bb2-ac7c-6b894e5402f3', 'VPN troubleshooting', 'vpn-troubleshooting', NULL, NULL, 'published', '2026-10-09 23:41:06.328022+00', '2026-10-09 23:41:06.328022+00', '01a1230a-afb4-76c2-946b-2607d67dd3e8', '01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, 0, false, false, NULL, NULL, NULL, NULL, false, NULL, NULL, NULL, NULL, 1, NULL);


--
-- Data for Name: documentation_collection_pages; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_collection_visibility; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_page_embeddings; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_page_tickets; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_page_visibility; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_revisions; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_starred_pages; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: documentation_subscriptions; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: email_suppressions; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: group_includes; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: idempotency_keys; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: import_jobs; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: knowledge_gaps; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: knowledge_gap_signals; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: linked_tickets; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.linked_tickets (ticket_id, linked_ticket_id, relation_type, description, created_at, created_by, workspace_id) VALUES (2, 1, 'duplicate_of', 'Duplicate', '2026-10-09 23:41:06.315751+00', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 1);


--
-- Data for Name: notification_types; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (1, 'ticket_assigned', 'Assigned to Ticket', 'When you are assigned to a ticket', 'ticket', '["in_app", "email"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (2, 'ticket_status_changed', 'Ticket Status Changed', 'When a ticket you are involved with changes status', 'ticket', '["in_app"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (3, 'comment_added', 'New Comment', 'When someone comments on a ticket you are involved with', 'comment', '["in_app"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (4, 'mentioned', 'Mentioned in Comment', 'When someone mentions you with @username', 'mention', '["in_app", "email"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (5, 'ticket_created_requester', 'Ticket Created', 'When a ticket is created where you are the requester', 'ticket', '["in_app"]', '2026-06-06 05:58:59.083307+00');
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (6, 'doc_page_updated', 'Documentation Page Updated', 'When a documentation page you subscribe to is modified', 'documentation', '["in_app"]', '2026-06-06 05:58:59.138446+00');
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (7, 'asset_low_stock', 'Asset Low Stock', 'When a stock-tracked asset''s quantity drops to or below its low-stock threshold', 'asset', '["in_app", "email"]', '2026-06-06 05:58:59.325193+00');
INSERT INTO public.notification_types (id, code, name, description, category, default_channels, created_at) VALUES (8, 'sla_breached', 'SLA Breached', 'When a ticket''s response or resolution SLA target has been missed', 'ticket', '["in_app", "email"]', '2026-06-06 05:58:59.462702+00');


--
-- Data for Name: notification_preferences; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: notification_rate_limits; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: notifications; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: outbound_emails; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: passkey_credentials; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: plugins; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: plugin_activity; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: plugin_collection_schemas; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: plugin_collection_rows; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: plugin_data; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: plugin_local_signing_key; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.plugin_local_signing_key (id, pubkey, encrypted_sk, fingerprint, created_at, encrypted_sk_kek_id) VALUES (1, '5GY1jilI+L6cDrzaymzy2K7Mh+/42HrQeKHRq3ibU2I=', '\x01010001f86efc1995853a26e2432c022f95cd0b4b4f7172034b9353a945bc30c273b8e290c5f2884b81b31136b3bd185ef5d638754f7420eca31053bce4586cbbba42285b296418c906ffdef211d161bc407e451cac5a300564e187afc9c3fb0e51f2e47282451bc0ddb343086156ad6c5ed0', '3c889810e10b0e3b', '2026-10-09 23:40:45.535254+00', 1);


--
-- Data for Name: plugin_registry_state; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.plugin_registry_state (id, publishers_version, index_version, last_fetched_at, last_fetch_error, updated_at) VALUES (1, 0, 0, NULL, 'registry signature verification failed', '2026-10-09 23:40:46.281698+00');


--
-- Data for Name: plugin_trusted_publishers; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: project_tickets; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: refresh_tokens; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.refresh_tokens (id, token_hash, user_uuid, created_at, expires_at, revoked_at, session_id, family_id, is_used, used_at, replaced_by_hash, grace_expires_at) VALUES (1, '4ca3136dd8a0a0624b1ce4010f7263cd0b63936c4786a7fdff4819a50e8d4b70', '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:06.164987+00', '2026-10-16 23:41:06.164493+00', NULL, '31b08365-0908-4b90-ac6f-f7e9f4cc997d', '2cc9d2d8-3e0f-465b-9aea-a6a38e826d77', false, NULL, NULL, NULL);


--
-- Data for Name: reset_tokens; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: retired_slugs; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: rules; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: rule_applications; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: rule_versions; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: saved_views; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: search_index_state; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.search_index_state (id, entity_type, last_indexed_at, index_version, document_count, last_error, last_error_at, created_at, updated_at) VALUES (1, 'ticket', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state (id, entity_type, last_indexed_at, index_version, document_count, last_error, last_error_at, created_at, updated_at) VALUES (2, 'comment', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state (id, entity_type, last_indexed_at, index_version, document_count, last_error, last_error_at, created_at, updated_at) VALUES (3, 'documentation', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state (id, entity_type, last_indexed_at, index_version, document_count, last_error, last_error_at, created_at, updated_at) VALUES (4, 'attachment', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state (id, entity_type, last_indexed_at, index_version, document_count, last_error, last_error_at, created_at, updated_at) VALUES (5, 'device', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');
INSERT INTO public.search_index_state (id, entity_type, last_indexed_at, index_version, document_count, last_error, last_error_at, created_at, updated_at) VALUES (6, 'user', NULL, 1, 0, NULL, NULL, '2026-06-06 05:58:59.115781+00', '2026-06-06 05:58:59.115781+00');


--
-- Data for Name: search_query_log; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: security_events; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: site_settings; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.site_settings (id, app_name, logo_url, logo_light_url, favicon_url, primary_color, created_at, updated_at, updated_by, guest_tickets_enabled, guest_public_docs_enabled, guest_kb_search_enabled, guest_ticket_lookup_enabled, guest_help_page_enabled, guest_ticket_default_priority, guest_ticket_rate_limit_per_hour, guest_ticket_email_verification, guest_ticket_attachments_enabled, guest_ticket_intro_message, channel_auto_ack_enabled, channel_auto_ack_template, feature_flags, default_locale, default_timezone, workspace_id, signature_default, email_security_note_enabled, email_security_note_template) VALUES (1, 'Nosdesk', NULL, NULL, NULL, NULL, '2026-06-06 05:58:59.018294+00', '2026-06-06 05:58:59.238807+00', NULL, false, false, false, false, false, NULL, 5, true, true, NULL, true, NULL, '{}', 'en-US', 'UTC', 1, NULL, false, NULL);


--
-- Data for Name: working_calendars; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.working_calendars (id, name, timezone, schedule, is_default, created_at, updated_at, created_by, workspace_id) VALUES (1, 'Default 9-5', 'UTC', '{"fri": [["09:00", "17:00"]], "mon": [["09:00", "17:00"]], "sat": [], "sun": [], "thu": [["09:00", "17:00"]], "tue": [["09:00", "17:00"]], "wed": [["09:00", "17:00"]]}', true, '2026-06-06 05:58:59.25774+00', '2026-06-06 05:58:59.25774+00', NULL, 1);


--
-- Data for Name: sla_policies; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.sla_policies (id, name, target_response_minutes, target_resolution_minutes, working_calendar_id, priority_filter, category_id_filter, is_default, created_at, updated_at, created_by, workspace_id, assignee_group_id_filter) VALUES (1, 'Default', 240, 1440, 1, NULL, NULL, true, '2026-06-06 05:58:59.25774+00', '2026-06-06 05:58:59.25774+00', NULL, 1, NULL);


--
-- Data for Name: sync_actions_2026_05; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions



--
-- Data for Name: sync_actions_2026_06; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions



--
-- Data for Name: sync_actions_2026_07; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions



--
-- Data for Name: sync_actions_2026_08; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions



--
-- Data for Name: sync_actions_2026_10; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions

INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (1, '01a1230a-afbb-72e3-9d19-4cc2eb02e971', 'user', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'I', 'user.created', 1, '{"name": "Fixture Admin", "uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "email": "admin@example.com", "pronouns": null, "avatar_url": null, "avatar_thumb": null, "platform_role": "platform_admin", "workspace_role": "admin"}', '{workspace:1}', NULL, 'system', 'admin_setup', NULL, NULL, NULL, '2026-10-09 23:41:05.340112+00', '2026-10-09 23:41:05.340112+00', 1, 755);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (2, '01a1230a-b305-72e2-95d3-a54cbbec2a6d', 'ticket', '1', 'I', 'ticket.created', 1, '{"id": 1, "uuid": "01a1230a-b303-7f09-9566-82f111145b63", "title": "Printer on level 3 jams", "due_date": null, "priority": "medium", "created_at": "2026-10-09T23:41:06.177313", "updated_at": "2026-10-09T23:41:06.177313", "category_id": null, "created_via": {"source": null, "subject": null, "from_name": null, "from_email": null}, "triage_state": null, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "workflow_state": {"id": 2, "name": "Backlog", "color": "gray", "category": "backlog"}, "last_activity_at": "2026-10-09T23:41:06.177313", "origin_channel_id": null, "workflow_state_id": 2}', '{workspace:1,ticket:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, 'fb7954f3-fe1a-49c0-9109-552bd6ce988f', NULL, NULL, '2026-10-09 23:41:06.181433+00', '2026-10-09 23:41:06.181433+00', 1, 763);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (3, '01a1230a-b310-749c-a8ea-20bf79286876', 'ticket', '2', 'I', 'ticket.created', 1, '{"id": 2, "uuid": "01a1230a-b30f-71ed-b15b-780e08e8f7bd", "title": "Printer level 3 jams again", "due_date": null, "priority": "medium", "created_at": "2026-10-09T23:41:06.189926", "updated_at": "2026-10-09T23:41:06.189926", "category_id": null, "created_via": {"source": null, "subject": null, "from_name": null, "from_email": null}, "triage_state": null, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "workflow_state": {"id": 2, "name": "Backlog", "color": "gray", "category": "backlog"}, "last_activity_at": "2026-10-09T23:41:06.189926", "origin_channel_id": null, "workflow_state_id": 2}', '{workspace:1,ticket:2}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, 'f95515d7-8ce3-4574-93ca-20c03e1c6216', NULL, NULL, '2026-10-09 23:41:06.192392+00', '2026-10-09 23:41:06.192392+00', 1, 765);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (4, '01a1230a-b316-7678-a05c-52afdfd23274', 'ticket', '3', 'I', 'ticket.created', 1, '{"id": 3, "uuid": "01a1230a-b316-7654-b700-543a9197e3d3", "title": "VPN drops every hour", "due_date": null, "priority": "medium", "created_at": "2026-10-09T23:41:06.197570", "updated_at": "2026-10-09T23:41:06.197570", "category_id": null, "created_via": {"source": null, "subject": null, "from_name": null, "from_email": null}, "triage_state": null, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "workflow_state": {"id": 2, "name": "Backlog", "color": "gray", "category": "backlog"}, "last_activity_at": "2026-10-09T23:41:06.197570", "origin_channel_id": null, "workflow_state_id": 2}', '{workspace:1,ticket:3}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '46173b4b-6295-4556-9ebd-ffd07ff90f8e', NULL, NULL, '2026-10-09 23:41:06.19831+00', '2026-10-09 23:41:06.19831+00', 1, 767);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (5, '01a1230a-b31b-7c6f-a37b-4cdb333e4cde', 'ticket', '4', 'I', 'ticket.created', 1, '{"id": 4, "uuid": "01a1230a-b31b-70ff-b517-f5425cbd8d1e", "title": "Laptop battery swollen", "due_date": null, "priority": "medium", "created_at": "2026-10-09T23:41:06.202522", "updated_at": "2026-10-09T23:41:06.202522", "category_id": null, "created_via": {"source": null, "subject": null, "from_name": null, "from_email": null}, "triage_state": null, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "workflow_state": {"id": 2, "name": "Backlog", "color": "gray", "category": "backlog"}, "last_activity_at": "2026-10-09T23:41:06.202522", "origin_channel_id": null, "workflow_state_id": 2}', '{workspace:1,ticket:4}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '08539c5e-439e-4bde-966d-8b57b8f6b788', NULL, NULL, '2026-10-09 23:41:06.203281+00', '2026-10-09 23:41:06.203281+00', 1, 769);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (6, '01a1230a-b332-7cc5-8de9-f74e40c0a2c0', 'attachment', '1', 'I', 'attachment.created', 1, '{"id": 1, "url": "/uploads/temp/01a1230a-b32f-75e3-8494-6ba9d6eb2497_note.txt", "name": "note.txt", "file_size": 19, "mime_type": "application/octet-stream", "comment_id": null}', '{workspace:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '3ff408e0-20df-4fdc-9e43-da559849fb4a', NULL, NULL, '2026-10-09 23:41:06.225644+00', '2026-10-09 23:41:06.225644+00', 1, 771);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (7, '01a1230a-b335-73c9-8c77-89fcd0ca3ff0', 'attachment', '2', 'I', 'attachment.created', 1, '{"id": 2, "url": "/uploads/temp/01a1230a-b334-7d41-a1df-53c3b6c42fca_pixel.png", "name": "pixel.png", "file_size": 70, "mime_type": "image/png", "comment_id": null}', '{workspace:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '3ff408e0-20df-4fdc-9e43-da559849fb4a', NULL, NULL, '2026-10-09 23:41:06.229066+00', '2026-10-09 23:41:06.229066+00', 1, 773);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (8, '01a1230a-b349-7823-ab02-0183aafeb933', 'comment', '1', 'I', 'comment.created', 1, '{"id": 1, "content": "<p>Logs attached</p>", "ticket_id": 1, "user_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "created_at": "2026-10-09T23:41:06.246844", "created_via": {"source": null, "from_name": null, "from_email": null}, "is_internal": false, "render_kind": "simple", "content_format": "html"}', '{workspace:1,ticket:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, 'b53f21c2-28cd-4c85-b6e5-704814493877', NULL, NULL, '2026-10-09 23:41:06.249388+00', '2026-10-09 23:41:06.249388+00', 1, 775);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (9, '01a1230a-b34f-7619-938d-4ccbec805d6a', 'ticket', '1', 'U', 'ticket.sla_updated', 1, '{"id": 1, "sla": {"paused": true, "breached": false, "response": {"met_at": "2026-10-09T23:41:06.246844Z", "paused": true, "breached": false, "start_at": "2026-10-09T23:41:06.177313Z", "target_at": "2026-10-12T13:00:00Z", "pill_color": "green", "seconds_remaining": null}, "start_at": "2026-10-09T23:41:06.177313Z", "target_at": "2026-10-14T17:00:00Z", "pill_color": "amber", "resolution": {"paused": true, "breached": false, "start_at": "2026-10-09T23:41:06.177313Z", "target_at": "2026-10-14T17:00:00Z", "pill_color": "amber", "seconds_remaining": 407933}, "seconds_remaining": 407933}, "first_response_at": "2026-10-09T23:41:06.246844"}', '{workspace:1,ticket:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, 'b53f21c2-28cd-4c85-b6e5-704814493877', NULL, NULL, '2026-10-09 23:41:06.254849+00', '2026-10-09 23:41:06.254849+00', 1, 775);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (10, '01a1230a-b353-73f5-b8b3-2938f3208b84', 'ticket', '1', 'U', 'ticket.watcher_added', 1, '{"ticket_id": 1, "user_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "auto_added": true}', '{workspace:1,ticket:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, 'b53f21c2-28cd-4c85-b6e5-704814493877', NULL, NULL, '2026-10-09 23:41:06.259513+00', '2026-10-09 23:41:06.259513+00', 1, 777);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (11, '01a1230a-b372-72ab-af98-91f8a700a862', 'ticket', '3', 'U', 'ticket.workflow_state_changed', 1, '{"id": 3, "sla": {"paused": true, "breached": false, "response": {"paused": true, "breached": false, "start_at": "2026-10-09T23:41:06.197570Z", "target_at": "2026-10-12T13:00:00Z", "pill_color": "amber", "seconds_remaining": 220733}, "start_at": "2026-10-09T23:41:06.197570Z", "target_at": "2026-10-12T13:00:00Z", "pill_color": "amber", "resolution": {"paused": true, "breached": false, "start_at": "2026-10-09T23:41:06.197570Z", "target_at": "2026-10-14T17:00:00Z", "pill_color": "amber", "seconds_remaining": 407933}, "seconds_remaining": 220733}, "title": "VPN drops every hour", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.287458", "closed_by": null, "created_by": null, "category_id": null, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "workflow_state": {"id": 5, "name": "Done", "color": "green", "category": "done"}, "resolution_notes": null, "origin_channel_id": null, "workflow_state_id": 5, "verification_state": null}', '{workspace:1,ticket:3}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, 'b7858158-e0af-4da3-add0-5907e193089f', NULL, NULL, '2026-10-09 23:41:06.290076+00', '2026-10-09 23:41:06.290076+00', 1, 781);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (12, '01a1230a-b380-7e25-af07-c5620997916a', 'ticket', '4', 'U', 'ticket.workflow_state_changed', 1, '{"id": 4, "sla": {"paused": true, "breached": false, "response": {"paused": true, "breached": false, "start_at": "2026-10-09T23:41:06.202522Z", "target_at": "2026-10-12T13:00:00Z", "pill_color": "amber", "seconds_remaining": 220733}, "start_at": "2026-10-09T23:41:06.202522Z", "target_at": "2026-10-12T13:00:00Z", "pill_color": "amber", "resolution": {"paused": true, "breached": false, "start_at": "2026-10-09T23:41:06.202522Z", "target_at": "2026-10-14T17:00:00Z", "pill_color": "amber", "seconds_remaining": 407933}, "seconds_remaining": 220733}, "title": "Laptop battery swollen", "due_date": null, "priority": "medium", "closed_at": "2026-10-09T23:41:06.302220", "closed_by": null, "created_by": null, "category_id": null, "assignee_uuid": null, "submitted_via": null, "requester_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "workflow_state": {"id": 5, "name": "Done", "color": "green", "category": "done"}, "resolution_notes": null, "origin_channel_id": null, "workflow_state_id": 5, "verification_state": null}', '{workspace:1,ticket:4}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '7f0e59a3-2c04-4a55-aaa0-f93c14d4e9b9', NULL, NULL, '2026-10-09 23:41:06.303902+00', '2026-10-09 23:41:06.303902+00', 1, 783);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (13, '01a1230a-b392-7664-9213-78eac6592949', 'comment', '2', 'I', 'comment.created', 1, '{"id": 2, "kind": "merge_marker", "content": "Merged 1 ticket(s) into this one:\n- #2: \"Printer level 3 jams again\"\nReason: Duplicate", "ticket_id": 1, "user_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "created_at": "2026-10-09T23:41:06.315751", "is_internal": false, "content_format": "html"}', '{workspace:1,ticket:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '6c5712b8-5be8-47c5-89ee-b5e5bfd2573e', NULL, NULL, '2026-10-09 23:41:06.321892+00', '2026-10-09 23:41:06.321892+00', 1, 785);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (14, '01a1230a-b392-7280-8676-66711b761606', 'ticket', '1', 'U', 'ticket.merged', 1, '{"reason": "Duplicate", "actor_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "comments_moved": 0, "watchers_added": 0, "customer_notified": false, "source_ticket_ids": [2], "merge_marker_comment_id": 2, "channel_messages_rerouted": 0}', '{workspace:1,ticket:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '6c5712b8-5be8-47c5-89ee-b5e5bfd2573e', NULL, NULL, '2026-10-09 23:41:06.322178+00', '2026-10-09 23:41:06.322178+00', 1, 785);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (15, '01a1230a-b392-7ee3-9577-768fd7213af7', 'ticket', '2', 'U', 'ticket.merged_into', 1, '{"id": 2, "merged_at": "2026-10-09T23:41:06.315751", "actor_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "merged_by_user_uuid": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "merged_into_ticket_id": 1}', '{workspace:1,ticket:2}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '6c5712b8-5be8-47c5-89ee-b5e5bfd2573e', NULL, NULL, '2026-10-09 23:41:06.322383+00', '2026-10-09 23:41:06.322383+00', 1, 785);
INSERT INTO public.sync_actions (sync_id, event_uuid, aggregate, aggregate_id, op, event_type, schema_version, data, groups, actor_uuid, actor_kind, actor_ref, correlation_id, causation_id, client_tx_id, occurred_at, recorded_at, workspace_id, xid8) VALUES (16, '01a1230a-b39b-7ba2-b6b3-952114641161', 'documentation_page', '1', 'I', 'documentation_page.created', 1, '{"id": 1, "icon": null, "slug": "vpn-troubleshooting", "uuid": "01a1230a-b398-7bb2-ac7c-6b894e5402f3", "title": "VPN troubleshooting", "status": "published", "is_public": false, "parent_id": null, "created_at": "2026-10-09T23:41:06.328022", "created_by": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "deleted_at": null, "updated_at": "2026-10-09T23:41:06.328022", "archived_at": null, "cover_image": null, "is_template": false, "verified_at": null, "verified_by": null, "collection_id": null, "display_order": 0, "last_edited_by": "01a1230a-afb4-76c2-946b-2607d67dd3e8", "verify_interval_days": null}', '{workspace:1}', '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'user', NULL, '901c842a-e569-426a-9fd8-ab396dd6907d', NULL, NULL, '2026-10-09 23:41:06.331021+00', '2026-10-09 23:41:06.331021+00', 1, 786);


--
-- Data for Name: sync_actions_2026_11; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions



--
-- Data for Name: sync_actions_2026_12; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions



--
-- Data for Name: sync_actions_default; Type: TABLE DATA; Schema: public; Owner: -
--

-- load via partition root public.sync_actions



--
-- Data for Name: sync_delta_tokens; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: sync_history; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: system_meta; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.system_meta (key, value, updated_at) VALUES ('sync_id_high_water', '0', '2026-06-06 05:58:59.217917+00');
INSERT INTO public.system_meta (key, value, updated_at) VALUES ('schema_hash', '"0319c33351b4a3c4"', '2026-10-09 23:40:45.50572+00');
INSERT INTO public.system_meta (key, value, updated_at) VALUES ('instance_id', '"6c1092a7-0239-409d-a0fe-d659bf4b2a12"', '2026-10-09 23:40:45.507414+00');
INSERT INTO public.system_meta (key, value, updated_at) VALUES ('partition_max_provisioned', '"2026-12-01"', '2026-10-09 23:40:45.532818+00');


--
-- Data for Name: tags; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: ticket_assets; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: ticket_rule_runs; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: ticket_tags; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: ticket_watchers; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.ticket_watchers (ticket_id, user_uuid, created_at, auto_added, notify_on_internal_notes, workspace_id) VALUES (1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', '2026-10-09 23:41:06.257467+00', true, true, 1);


--
-- Data for Name: user_auth_identities; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.user_auth_identities (id, user_uuid, provider_type, external_id, email, metadata, password_hash, created_at, updated_at, created_by) VALUES (1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'local', 'admin@example.com', 'admin@example.com', NULL, '$2b$12$9daKhjdhIo7E8wrqJDtpP.kwlq.LB3dW7A/TC.htmR6vkVb66PmoW', '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:05.333158+00', NULL);


--
-- Data for Name: user_emails; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.user_emails (id, user_uuid, email, email_type, is_primary, is_verified, source, created_at, updated_at, created_by) VALUES (1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'admin@example.com', 'personal', true, true, 'manual', '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:05.333158+00', NULL);


--
-- Data for Name: user_groups; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: user_preferences; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.user_preferences (user_uuid, theme, signature, dashboard_layout, locale, timezone, created_at, updated_at) VALUES ('01a1230a-afb4-76c2-946b-2607d67dd3e8', NULL, NULL, NULL, NULL, NULL, '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:05.333158+00');


--
-- Data for Name: user_recovery_codes; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: user_ticket_views; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: webhooks; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: webhook_deliveries; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: webhook_outbox; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (2, '2026-10-09 23:41:06.177313+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (3, '2026-10-09 23:41:06.189926+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (4, '2026-10-09 23:41:06.19757+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (5, '2026-10-09 23:41:06.202522+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (6, '2026-10-09 23:41:06.224489+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (7, '2026-10-09 23:41:06.228565+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (8, '2026-10-09 23:41:06.246844+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (9, '2026-10-09 23:41:06.246844+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (10, '2026-10-09 23:41:06.257467+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (11, '2026-10-09 23:41:06.287753+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (12, '2026-10-09 23:41:06.30224+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (13, '2026-10-09 23:41:06.315751+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (14, '2026-10-09 23:41:06.315751+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (15, '2026-10-09 23:41:06.315751+00');
INSERT INTO public.webhook_outbox (sync_id, enqueued_at) VALUES (16, '2026-10-09 23:41:06.328022+00');


--
-- Data for Name: working_calendar_holidays; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Data for Name: workspace_members; Type: TABLE DATA; Schema: public; Owner: -
--

INSERT INTO public.workspace_members (workspace_id, user_uuid, role, invited_at, accepted_at) VALUES (1, '01a1230a-afb4-76c2-946b-2607d67dd3e8', 'admin', '2026-10-09 23:41:05.333158+00', '2026-10-09 23:41:05.333158+00');


--
-- Data for Name: yjs_snapshots; Type: TABLE DATA; Schema: public; Owner: -
--



--
-- Name: active_sessions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.active_sessions_id_seq', 2, false);


--
-- Name: api_tokens_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.api_tokens_id_seq', 1, false);


--
-- Name: article_content_revisions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.article_content_revisions_id_seq', 1, false);


--
-- Name: article_contents_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.article_contents_id_seq', 1, false);


--
-- Name: asset_audits_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.asset_audits_id_seq', 1, false);


--
-- Name: asset_kinds_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.asset_kinds_id_seq', 14, false);


--
-- Name: asset_lifecycle_events_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.asset_lifecycle_events_id_seq', 1, false);


--
-- Name: asset_media_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.asset_media_id_seq', 1, false);


--
-- Name: asset_usage_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.asset_usage_log_id_seq', 1, false);


--
-- Name: assignment_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.assignment_log_id_seq', 1, false);


--
-- Name: assignment_rules_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.assignment_rules_id_seq', 1, false);


--
-- Name: attachments_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.attachments_id_seq', 3, false);


--
-- Name: audit_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.audit_log_id_seq', 46, false);


--
-- Name: bug_reports_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.bug_reports_id_seq', 1, false);


--
-- Name: canned_response_insertions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.canned_response_insertions_id_seq', 1, false);


--
-- Name: canned_responses_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.canned_responses_id_seq', 1, false);


--
-- Name: channel_credentials_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.channel_credentials_id_seq', 1, false);


--
-- Name: channel_messages_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.channel_messages_id_seq', 1, false);


--
-- Name: channels_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.channels_id_seq', 1, false);


--
-- Name: comments_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.comments_id_seq', 3, false);


--
-- Name: csp_reports_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.csp_reports_id_seq', 1, false);


--
-- Name: cycles_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.cycles_id_seq', 1, false);


--
-- Name: devices_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.devices_id_seq', 1, false);


--
-- Name: documentation_collection_visibility_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.documentation_collection_visibility_id_seq', 1, false);


--
-- Name: documentation_collections_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.documentation_collections_id_seq', 3, false);


--
-- Name: documentation_page_visibility_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.documentation_page_visibility_id_seq', 1, false);


--
-- Name: documentation_pages_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.documentation_pages_id_seq', 2, false);


--
-- Name: documentation_revisions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.documentation_revisions_id_seq', 1, false);


--
-- Name: documentation_starred_pages_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.documentation_starred_pages_id_seq', 1, false);


--
-- Name: documentation_subscriptions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.documentation_subscriptions_id_seq', 1, false);


--
-- Name: groups_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.groups_id_seq', 1, false);


--
-- Name: knowledge_gap_signals_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.knowledge_gap_signals_id_seq', 1, false);


--
-- Name: knowledge_gaps_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.knowledge_gaps_id_seq', 1, false);


--
-- Name: notification_preferences_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.notification_preferences_id_seq', 1, false);


--
-- Name: notification_rate_limits_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.notification_rate_limits_id_seq', 1, false);


--
-- Name: notification_types_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.notification_types_id_seq', 9, false);


--
-- Name: notifications_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.notifications_id_seq', 1, false);


--
-- Name: outbound_emails_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.outbound_emails_id_seq', 1, false);


--
-- Name: plugin_activity_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.plugin_activity_id_seq', 1, false);


--
-- Name: plugin_collection_rows_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.plugin_collection_rows_id_seq', 1, false);


--
-- Name: plugin_collection_schemas_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.plugin_collection_schemas_id_seq', 1, false);


--
-- Name: plugin_data_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.plugin_data_id_seq', 1, false);


--
-- Name: plugin_trusted_publishers_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.plugin_trusted_publishers_id_seq', 1, false);


--
-- Name: plugins_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.plugins_id_seq', 1, false);


--
-- Name: projects_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.projects_id_seq', 1, false);


--
-- Name: refresh_tokens_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.refresh_tokens_id_seq', 2, false);


--
-- Name: rule_applications_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.rule_applications_id_seq', 1, false);


--
-- Name: rule_versions_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.rule_versions_id_seq', 1, false);


--
-- Name: rules_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.rules_id_seq', 1, false);


--
-- Name: saved_views_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.saved_views_id_seq', 1, false);


--
-- Name: search_index_state_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.search_index_state_id_seq', 7, false);


--
-- Name: search_query_log_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.search_query_log_id_seq', 1, false);


--
-- Name: security_events_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.security_events_id_seq', 1, false);


--
-- Name: site_settings_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.site_settings_id_seq', 2, false);


--
-- Name: sla_policies_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.sla_policies_id_seq', 2, false);


--
-- Name: sync_actions_sync_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.sync_actions_sync_id_seq', 17, false);


--
-- Name: sync_delta_tokens_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.sync_delta_tokens_id_seq', 1, false);


--
-- Name: sync_history_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.sync_history_id_seq', 1, false);


--
-- Name: tags_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.tags_id_seq', 1, false);


--
-- Name: ticket_categories_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.ticket_categories_id_seq', 5, false);


--
-- Name: tickets_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.tickets_id_seq', 5, false);


--
-- Name: user_auth_identities_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.user_auth_identities_id_seq', 2, false);


--
-- Name: user_emails_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.user_emails_id_seq', 2, false);


--
-- Name: user_recovery_codes_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.user_recovery_codes_id_seq', 1, false);


--
-- Name: user_ticket_views_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.user_ticket_views_id_seq', 1, false);


--
-- Name: webhook_deliveries_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.webhook_deliveries_id_seq', 1, false);


--
-- Name: webhooks_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.webhooks_id_seq', 1, false);


--
-- Name: workflow_states_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.workflow_states_id_seq', 8, false);


--
-- Name: working_calendar_holidays_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.working_calendar_holidays_id_seq', 1, false);


--
-- Name: working_calendars_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.working_calendars_id_seq', 2, false);


--
-- Name: workspaces_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.workspaces_id_seq', 2, false);


--
-- Name: yjs_snapshots_id_seq; Type: SEQUENCE SET; Schema: public; Owner: -
--

SELECT pg_catalog.setval('public.yjs_snapshots_id_seq', 1, false);


--
-- PostgreSQL database dump complete
--


