# Scripting implementation report

Implemented in the existing backend, dashboard and desktop companion worktrees. Existing CS2 normalization, signature verification, auth stripping and live Wingman regression coverage were preserved. No repository was reset; the pre-existing backend .idea directory was left alone. Backend minor version: 0.10.0 (previously 0.9.3). Delivery uses one implementation commit per repository: backend master, frontend main and companion main, following each repository's existing commit format.

## Architecture and behavior

- Rhai 1.26.1 with sync/serde, a revision-only resolver, pure module initialization, instruction/depth/container/deadline limits, host budgets and two bounded execution workers. There is no tokenizer adapter, source preprocessing, filesystem/network/process access, eval or user-native extension loading. Native names are ctx.current_match, UserFilter::create() and RewardFilter::create(). Native reserved-word diagnostics are tested.
- Per-channel projects own versioned drafts, immutable published snapshots, handler metadata, KV storage, jobs and reports. Save is separate from validate/publish; stale saves fail. Failed publish preserves the current revision; rollback activates a previous snapshot.
- Durable event/timer execution is serialized per project while other projects can progress. Events share before/after snapshots. Timers are pinned to their creating revision. Disable preserves jobs and blocks expiry; re-enable does not run accumulated overdue jobs. Confirmed manual execution can run a disabled project's pinned job exactly once. Cancellation and interrupted executions retain history; ambiguous external effects are not replayed.
- Match data is shared per channel, with an active observation plus 30 completed observations. Round records preserve known stats, scores, side, state and semantic timelines; gaps/unknown data are not filled with invented values. Scripts get compact round history; Matches exposes full retained timelines.
- Host APIs cover Duration; rewards get/visibility/pause/temporary enable/trigger; chat/users statistics and builder filters; chat send/reply; scheduler after/cancel/exists; atomic project storage; random pick; and four log levels. See README.md for signatures, result codes, context shapes and all numeric quotas.
- Reward aliases are channel-scoped identifiers. is_visible is independent of pause/pricing/fulfillment and maps to Helix is_enabled. Storefront inclusion has its own setting. Internal SCRIPT fulfillment bypasses Twitch-specific cooldown/per-stream limits, while preserving chat requirements, explicit purchase limits, item selection, inventory/buyer and Market checks. It spends zero points and has no Twitch redemption ID.
- SCRIPT inventory uses confirmed owner-only, audited discard in safe states. Unresolved orders/trades block discard. Existing TWITCH refund behavior remains. Script trigger outcomes never automatically send Twitch messages, including later reconciliation.
- General audit retains real actions and script correlation, including downstream Market events. Human manual actions retain human attribution. Separate script reports contain authored logs, compile/runtime/host errors, planned actions, results, metadata and timings, with documented retention.
- Dashboard Scripts contains CS2 Integration, Editor, Local Storage, Matches, Scheduler and Logs. It uses Monaco (@monaco-editor/react + locally bundled monaco-editor) and react-arborist with existing dashboard components. File paths use virtual model URIs; drafts have tabs, dirty indicators, save/validate/publish/rollback and diagnostics navigation. Completion/hover covers native host names, context fields, filters and results. Recorded-event dry runs have no domain side effects.
- The companion adds phase_countdowns to its existing GSI whitelist. Exact owned legacy/current configurations are removable; user-edited configurations remain protected.

## Migrations and rollout

Five migrations were added:

1. 20260928010000_scripting.sql: project/revision/storage/job/match/snapshot/execution tables.
2. 20260928020000_reward_scripting.sql: visibility and unique scoped aliases.
3. 20260928030000_fulfillment_origin.sql: fulfillment identity, actual nullable Twitch identity, SCRIPT attribution and discard state.
4. 20260928040000_script_editor_reports.sql: compile/test report history.
5. 20260928050000_script_audit_attribution.sql: downstream audit correlation, suppression of false Twitch-fulfillment pending events and a script rate-query index.

Backend and frontend must be deployed together because the internal API ID is fulfillment_id and twitch_redemption_id now means the actual nullable Twitch ID. Existing IDs and referencing foreign keys are preserved. The existing SQLx migration runner applies the schema; production was not migrated during this task.

## Verification

- Backend: 124 tests passed on a fresh disposable PostgreSQL 17 database, including all ignored database integration tests. cargo check passed. Clippy completed with warnings; new scripting modules pass rustfmt checks. Existing repository-wide Rust formatting was not rewritten.
- Browser: all 70 tests passed in the final full-suite run. Focused scripting tests passed three consecutive runs (12/12) after fixing a rapid-typing/save race; the test now asserts that the entire typed text is saved. Earlier targeted CS2/inventory/scripting coverage passed 22/22.
- Frontend TypeScript, lint and production build passed. Lint retains existing warnings. Vite reports the large lazy Monaco editor chunk (about 739 KiB gzip); the worker is bundled locally without a CDN.
- Companion: 10 tests passed; one pre-existing Windows Credential Manager integration test remains intentionally ignored. Clippy and formatting checks passed.
- Reviewed the three repository diffs and new runtime/migrations, inspected desktop/mobile editor screenshots, and checked whitespace. Review fixes included project lock compatibility, automatic chat suppression, downstream audit correlation, bounded queue admission, disabled timer races, native Rhai syntax, and Monaco buffer ownership.
- No live Twitch reward changes, chat messages, production DB migrations or paid Market purchases were used for verification. Browser APIs were mocked; external delivery behavior still needs normal staging/live-service acceptance testing.

New coverage includes module paths/import cycles/missing files, unavailable capabilities and budgets, native Rhai syntax/diagnostics, the complete documented multi-file example, ownership, optimistic drafts, failed publish/rollback, disabled and revision-pinned jobs, one-shot manual runs/cancellation, project isolation/concurrency, atomic storage/null overlays, dry-run isolation, recorded contexts, chat stats, origin/points attribution, explicit domain limits, invisible reward processing, unsafe discard rejection, downstream audit/chat suppression, shared semantic snapshots, match retention, mobile editing and confirmed discard.

## Explicit limitations and differences from the illustrative specification

- Pseudo-code spellings use native Rhai alternatives rather than token rewriting. The normalized CS2 state's original match key remains accessible as ctx.state["match"]. Filter statuses use the existing uppercase domain states rather than invented accepted/delivered aliases.
- Full semantic timelines are retained in Matches. They are omitted from repeated execution snapshots to bound duplication; script round stats/state/history remain available. Only reliably observed rounds are recorded, and incomplete final rounds are not synthesized as completed.
- The editor provides static API completion/hover and returned compiler diagnostics, not a full language server or inferred arbitrary variable types. Move is an explicit rename/path action, not drag/drop. Empty folders use a module placeholder. New scripting UI localization is not complete.
- The API exposes bounded recent windows rather than pagination through all retained history. Scripting command schemas are documented in README.md rather than generated OpenAPI declarations.
- Dry-run Market/buyer outcomes cannot be guaranteed without side effects. Reward/scheduler writes are planned actions; subsequent reads reflect real state. Storage alone has a private read-after-write overlay. Random selection is not deterministic.
- There is no OS-enforced heap ceiling or separate sandbox process, compiled AST cache, KV TTL, project duplication UI or speculative subscription/user capabilities. Rhai limits, source/storage/queue caps, explicit host capabilities and bounded workers provide the implemented controls.
- Histories and source revisions are bounded as documented. A project reaching 200 revisions requires a new project to preserve pinned history. External effects are not transactionally atomic with PostgreSQL; timeouts/interruption are surfaced as ambiguous and never blindly replayed.
- Aliases/visibility are editable immediately after reward creation; they are not extra fields in the existing creation wizard.

## Exact changed files

Paths below are relative to each repository. Generated target/dist/test-results/.qa artifacts are not source changes.

### necko7

- Added: `docs/scripting/IMPLEMENTATION_REPORT.md`
- Added: `docs/scripting/README.md`
- Added: `docs/scripting/examples/events/giveaway.rhai`
- Added: `docs/scripting/examples/events/rounds.rhai`
- Added: `docs/scripting/examples/main.rhai`
- Added: `docs/scripting/examples/timers/router.rhai`
- Added: `migrations/20260928010000_scripting.sql`
- Added: `migrations/20260928020000_reward_scripting.sql`
- Added: `migrations/20260928030000_fulfillment_origin.sql`
- Added: `migrations/20260928040000_script_editor_reports.sql`
- Added: `migrations/20260928050000_script_audit_attribution.sql`
- Added: `src/scripting/api.rs`
- Added: `src/scripting/matches.rs`
- Added: `src/scripting/mod.rs`
- Added: `src/scripting/runtime.rs`
- Added: `src/scripting/service.rs`
- Added: `src/scripting/tests.rs`
- Added: `src/scripting/worker.rs`
- Modified: `Cargo.lock`
- Modified: `Cargo.toml`
- Modified: `README.md`
- Modified: `src/api/v1/chat_stats.rs`
- Modified: `src/api/v1/cs2.rs`
- Modified: `src/api/v1/cs2_tests.rs`
- Modified: `src/api/v1/mod.rs`
- Modified: `src/api/v1/public_broadcasters.rs`
- Modified: `src/api/v1/redemptions.rs`
- Modified: `src/api/v1/rewards.rs`
- Modified: `src/api/v1/viewer_profile.rs`
- Modified: `src/db/broadcaster_settings.rs`
- Modified: `src/db/inventory.rs`
- Modified: `src/db/redemptions.rs`
- Modified: `src/db/rewards.rs`
- Modified: `src/helix/api/custom_rewards/mod.rs`
- Modified: `src/helix/api/custom_rewards/model.rs`
- Modified: `src/main.rs`
- Modified: `src/processor/inventory_fulfillment.rs`
- Modified: `src/processor/lifecycle.rs`
- Modified: `src/processor/redemption.rs`

### necko7-frontend

- Added: `src/lib/scriptLanguage.ts`
- Added: `src/pages/ScriptsPage.tsx`
- Added: `tests/scripts.spec.ts`
- Modified: `package-lock.json`
- Modified: `package.json`
- Modified: `src/App.tsx`
- Modified: `src/components/layout/AppLayout.tsx`
- Modified: `src/components/profiles/InventoryList.tsx`
- Modified: `src/components/profiles/ViewerHistory.tsx`
- Modified: `src/components/redemptions/RedemptionCase.tsx`
- Modified: `src/components/redemptions/RedemptionList.tsx`
- Modified: `src/lib/inventoryLabels.ts`
- Modified: `src/pages/DashboardPage.tsx`
- Modified: `src/pages/RewardsPage.tsx`
- Modified: `src/pages/SettingsPage.tsx`
- Modified: `src/types/api.ts`
- Modified: `tests/consistency.spec.ts`
- Modified: `tests/correction-fixtures.ts`
- Modified: `tests/fixtures.ts`
- Modified: `tests/inventory.spec.ts`
- Modified: `tests/observability.spec.ts`

### necko7-cs2i

- Modified: `src-tauri/src/gsi_config.rs`
