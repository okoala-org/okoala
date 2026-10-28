# Data model outline

This is an outline of the v1 schema in PostgreSQL. The full schema is designed as a whole
for every protocol in scope and reviewed as a document before SQL is written. Every tenant
table leads with `tenant_id`; account tables lead with `tenant_id, account_id`
(invariant I1 in [invariants](invariants.md)). Blob bytes live in S3, not in the
database.

| Group | Tables |
| --- | --- |
| Platform and directory | `installation`, `tenants` (with the primary chat domain, fixed at creation), `tenant_policies` (sharing, retention, chat enabled, federation mode and allow-list), `tenant_limits`, `domains`, `dkim_keys`, `accounts` (identity id, state, quota, `used_bytes`, `last_seq`, `min_change_seq`), `addresses`, `groups`, `group_members`, `credentials` (app passwords: hash, protocol scopes, expiry), `admin_roles`, `audit_log` |
| Chat | `chat_localparts` (unique per tenant), `chat_instances` (server name, API host, DNS mode and verification state, Hydra client id, desired Tuwunel digest, resource overrides, canary flag, observed state, last health check, last successful backup, retention deadline), `chat_group_rooms` |
| State | `object_states`, `change_log`, `membership_log` |
| Infrastructure | `jobs`, `rate_counters` (unlogged), `blobs`, `uploads`, `mta_sts_cache`, `greylist` |
| Mail | `mailboxes` (UIDVALIDITY, UIDNEXT, highest MODSEQ, role, parent), `mailbox_subscriptions`, `emails` (blob, size, received time, thread, header index, body structure, preview), `email_keywords`, `email_mailbox` (UID, MODSEQ, `\Deleted`), `threads`, `thread_refs`, `identities`, `vacation_responses`, `email_submissions`, `queue_messages`, `queue_recipients`, `search_state` |
| Calendar | `calendars`, `calendar_events` (JSCalendar object, UID, sequence, UTC bounds), `calendar_event_members`, `event_occurrences`, `event_horizons`, `event_user_props` (per-sharee alerts, colour, keywords), `alarm_triggers`, `participant_identities`, `calendar_event_notifications`, `itip_messages` |
| Contacts | `addressbooks`, `contact_cards`, `contact_card_members`, `contact_card_emails` |
| Sharing | `sharing_grants` (collection, grantee user or group, grantee tenant, rights), `share_notifications` |
| DAV | `dav_names` (unique per collection), `dav_properties` (bounded dead properties) |
| Push | `push_subscriptions` (JMAP Web Push), `push_devices` (Matrix push gateway, with a unique server-generated `push_key`) |

## Notes

- **States.** `accounts.last_seq` is the account's sequence; `object_states` holds the
  state per object type; `change_log` and `membership_log` record what changed at each
  sequence number ([0014](../adr/0014-concurrency-and-state-model.md)).
- **IMAP.** `\Deleted` is per mailbox membership; other flags are per-email keywords, as
  in JMAP. A mailbox's highest MODSEQ is stored on its row.
- **Quota.** `used_bytes` covers mail, events, cards and uploads.
- **Occurrences.** `event_occurrences` covers a rolling horizon of 12 months back and 18
  months ahead; `event_horizons` records each event's materialised range and whether its
  rule is dense ([0010](../adr/0010-calendar-data-model.md)).
- **Blobs.** `blobs` records hash, size, backend, state (`live` or `deleting`) and the
  timestamps garbage collection needs; `emails`, `uploads`, `queue_messages` and import
  staging reference it.
- **Rate counters** are `UNLOGGED`: they are shared by every replica and need not survive
  a crash.
- **Secrets at rest.** App passwords are stored only as password hashes. DKIM private keys
  are stored encrypted; only their public half is exported.
