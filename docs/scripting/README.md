# Scripting

Scripts is the channel owner's automation workspace. Projects share channel CS2 state, match history, rewards and chat data, but own their files, revisions, storage, timers and execution history. Open **Scripts > Editor**, create a project, edit its draft, save, validate, publish, then enable it. New projects start disabled.

## Drafts and published revisions

Ctrl+S saves the draft only. A save carries a draft version; stale editors receive a conflict instead of overwriting someone else's work. Validation compiles every file and its imports, including files not imported by main.rhai. Publishing validates again under the project lock, creates an immutable revision and atomically makes it active. A failed publication preserves the previous active revision. Rollback activates an older revision; it does not overwrite the draft or repin timers.

Files are relative ASCII .rhai paths. Imports are project-root-relative, optionally without the extension: `import "events/rounds" as rounds;`. There is no filesystem resolver. Folders are implicit in file paths; creating an empty folder creates `_folder.rhai`. Rename/move does not rewrite imports: update the affected imports and validate. The tree supports explicit rename/move; drag/drop is disabled. Module initialization must be pure; call host capabilities inside handlers/functions.

The editor uses local Monaco models with a distinct virtual URI per channel/project/file, Rhai highlighting, completion and hover documentation. Completion covers the host API and common context/result/filter names. It is a lightweight language provider, not a full Rhai language server: arbitrary variable types are not inferred. Diagnostics include file/line information when supplied by Rhai, with a jump action.

## Entrypoints and normalized contexts

```rhai
fn on_event(ctx) {
    if ctx.event.kind == "player_kill" { log.info("Observed local kill"); }
}
fn on_timer(ctx) {
    log.info(`Timer: ${ctx.timer.key}`);
}
```

Publishing records `has_on_event` and `has_on_timer`. Projects without the appropriate handler are not dispatched that source. on_event receives all semantic CS2 events; there is no subscription UI. Events retain the existing normalizer's kinds, ordering and uncertainty rules.

| Field | Meaning |
| --- | --- |
| ctx.event | One normalized semantic event, including kind and source identifiers |
| ctx.state | Normalized state after the GSI payload |
| ctx.previous | State before that payload, or () when unknown |
| ctx.current_match | Shared match context, or () outside a known match |
| ctx.meta | execution_id, project_id, channel_id, revision, timestamp, source metadata |
| ctx.timer | on_timer only: id, key, payload, created_at, scheduled_for, creating_revision |

All events from one payload reference the same persisted before/after snapshot. No Valve `previously`/`added` interpretation is performed by scripting. No credentials, GSI authentication token or signing keys enter these contexts. `ctx.state.player` is unknown unless the normalizer proves the local player's identity.

`ctx.current_match.rounds` holds observed completed rounds, `current_round` the latest observed round, and `last_rounds(n)` returns the last n completed observed rounds (maximum 256). Round records include observed index, timestamps, completion, score before/after, winner, player kills/headshot kills/side/health, cumulative match_stats, and start/end player state. Damage dealt and per-round assists are not invented. Unknown values serialize to Rhai `()`, not zero. Match history stores semantic event timelines; execution snapshots omit those repeated timelines to keep dispatch compact. Missing observations can leave gaps: check round indices when requiring consecutive game rounds.

There is one active match per channel and at most 30 completed matches, shared across projects. Map/session/device changes and observed match restarts close the previous observation with `end_reason=observation_reset`. This is observation history, not a claim that a disconnected match completed normally. Only observed RoundEnded semantics complete round records; incomplete final rounds remain visibly incomplete. No raw GSI stream is stored here.

## Host API

Time APIs use `Duration::from_secs`, `from_mins`, `from_hours`, `from_days`, or `from_weeks` with a positive integer, up to one year. Chat/filter windows require at least one minute. The existing leaderboard REST API also accepts `window_minutes` (1..525600), taking precedence over `time_window_hours`; scripts call Rust queries directly.

| Capability | Operations |
| --- | --- |
| rewards | get(alias), set_visible(alias, bool), set_paused(alias, bool), enable_for(alias, Duration), trigger(alias, user_id) |
| chat / users | recent_chatters(Duration, UserFilter), user_stats(user_id, Duration) |
| chat | send(message), reply(message_id, message) |
| scheduler | after(key, Duration, payload), exists(key), cancel(key) |
| storage | get(key), get(key, default), set(key, value), delete(key), increment(key, integer) |
| random | pick(array), returning () for an empty array |
| log | debug(message), info(message), warn(message), error(message) |

Storage accepts JSON-like null, booleans, numbers, strings, arrays and maps; native host objects cannot be persisted. `increment` is an atomic checked integer increment, starting at zero if absent. Storage is project-scoped. Editor storage changes take the same quota lock. TTL is not implemented.

Chat users expose id/login, message and character counts and first/last activity. user_stats adds redemption totals, completed count and SCRIPT count for the requested window; absent activity has zero counts and unknown timestamps. Queries return at most 1,000 users. Filters compose without positional argument growth:

```rhai
let filter = UserFilter::create()
    .min_messages(5)
    .min_characters(300)
    .reward_redemptions(RewardFilter::create()
        .reward("random_skin")
        .statuses(["COMPLETED"])
        .min_count(2)
        .during(Duration::from_days(7)));
let users = chat.recent_chatters(Duration::from_hours(2), filter);
```

Statuses use the existing domain names: PENDING, MANUAL_HOLD, ORDER_CREATED, COMPLETED, FAILED_REFUND, FAILED_PENALTY. Subscription information and other unavailable user data are not fabricated. The API uses native Rhai syntax: `current_match` and `create()` avoid reserved words. There is no preprocessing or token rewriting. The underlying normalized CS2 state retains its existing schema; access its map/mode/score field with `ctx.state["match"]` (and `ctx.previous["match"]` when previous exists).

## Rewards, visibility and fulfillment

Set an optional **script alias** in the reward editor. Aliases must match `[a-z][a-z0-9_]{0,63}` and be unique among non-deleted rewards in the channel. Empty clears it. New rewards can be configured immediately after creation.

`is_visible` controls presentation, mapped to Twitch Helix `is_enabled`. It does not pause processing, price updates, market rules or script triggers. `is_paused` remains operational pause state. Existing rewards migrate to visible. The storefront setting `public_rewards_config.show_invisible_rewards` independently decides whether hidden rewards appear in the necko7 storefront. It defaults false.

`rewards.trigger` invokes the existing internal fulfillment pipeline. It creates a real internal fulfillment with `origin=SCRIPT`, null Twitch redemption ID, zero Twitch point cost, and project/revision/execution attribution. No EventSub message or Twitch redemption is fabricated. It bypasses Twitch cooldown and per-stream/per-user-per-stream limits, while preserving channel/reward operational state, chat requirements, explicit purchase_limits, atomic admission, item selection, inventory ownership, buyer/trade-link checks and market constraints. An admitted item may await the viewer or operator; success does not mean Steam delivery is complete.

A result includes `ok`, `code` when rejected, and available fulfillment_id, inventory_item_id, selected_item, origin and inventory_status. Stable codes include:

| Code | Meaning |
| --- | --- |
| reward_not_found / user_not_found | Alias is not in this channel / user has neither an observed channel chat identity nor a known account |
| reward_paused / bot_or_reward_inactive | Operational processing unavailable |
| activity_requirement_failed | Reward's chat requirement failed |
| purchase_limit_reached | Explicit necko7 purchase limit reached |
| trigger_rate_limit | Project exceeded 10 attempts per minute; retry_after=60 |
| trade_link_required | Inventory exists; viewer must configure delivery |
| market_unavailable / market_rejected | Purchase cannot currently proceed; inspect inventory |
| fulfillment_pending | Selection/order outcome is incomplete or requires reconciliation; do not blindly trigger again |
| price_limit | Configured item price constraint rejected selection |
| service_unavailable / host_timeout | Host did not complete; effects may already have occurred, inspect history |

Trigger results and errors never automatically go to Twitch chat, including later inventory reconciliation. The script may explicitly send a message. SCRIPT items display their origin. Their owner can confirm Discard in safe waiting/retry states; an unresolved order/trade blocks it. Discard transitions to DISCARDED, records actor/time and keeps fulfillment/audit history. It never returns points. TWITCH items retain their points-refund behavior. Operator refund/penalty endpoints reject SCRIPT items.

## Timers and disabled projects

```rhai
// In on_event, with on_timer defined in this published project:
rewards.set_visible("triple_triple", true);
scheduler.after("hide_reward", Duration::from_secs(333), #{reward: "triple_triple"});
// In on_timer:
if ctx.timer.key == "hide_reward" {
    rewards.set_visible(ctx.timer.payload.reward, false);
}
```

Every job keeps its creating revision and payload. Publishing or rolling back does not change them. Stable keys are project-local; scheduling the same pending key cancels the old record with reason `replaced` and retains history. Already queued work cannot be replaced. `enable_for` uses a persisted internal hide-reward job and needs no on_timer handler; it never sleeps a worker. Disabling also blocks this automatic reversal, so visibility can remain open until a manual run or explicit change.

Disable preserves files, storage, logs and jobs. Due jobs become blocked with project_disabled. Re-enable never auto-runs expired blocked jobs, even between scheduler polls. Future jobs resume their normal due behavior. **Run now** requires confirmation, can run a disabled project's exact pinned revision and consumes the one-shot job. Cancel preserves history. There is no Leave as is action.

Two bounded workers can process different projects concurrently; a project row lock serializes its executions. Durable queue markers survive restart. An interrupted execution is marked interrupted and never replayed automatically because external side effects may already exist. The existing inventory reconciliation continues handling ambiguous purchases. There is no distributed transaction spanning Twitch, Market and PostgreSQL: host_timeout is an ambiguous outcome, not proof of no effects.

## Dry run and logs

Save a draft, choose on_event/on_timer, then enter normalized JSON or load a recent recorded CS2 event. Dry run executes the draft and records would-be actions and logs, without sending chat, buying, changing visibility, scheduling jobs or mutating inventory/storage. Storage has a private read-after-write overlay. Reward validation checks chat and current purchase counts without reserving capacity. Market/buyer responses cannot be guaranteed without real calls and are explicitly marked unsimulated. Scheduler/reward writes are recorded plans; subsequent reads still reflect live state. Random picks remain random.

Scripts > Logs combines durable execution reports and editor compile/test diagnostics, with project, status, source, level and text filters. Reports include errors, actions, duration and correlation IDs. General channel logs record real actions separately. Fulfillment audit events inherit script attribution, including subsequent market work; explicit human actors remain human. Reports retain 30 days, terminal unreferenced jobs 30 days, and unreferenced event snapshots seven days, pruned hourly in bounded batches. Revision snapshots are retained for rollback and pinned jobs.

## Limits and operations

| Resource | Limit |
| --- | --- |
| Projects per channel / revisions per project | 20 / 200 |
| Source files / total source / path | 64 / 256 KiB / 180 characters |
| Instructions / call stack / expression depths | 100,000 / 32 / 64 global, 32 function |
| Execution deadline / host operation timeout | 3 seconds / at most 2 seconds and remaining execution budget |
| String / array / map / modules / import depth | 64 KiB / 2,048 / 512 / 64 / 16 |
| Host calls | 100 total, 50 per general operation |
| Trigger / chat.send / chat.reply | 3 each per execution; triggers also 10 per project/minute |
| Each reward mutation operation | 10 per execution |
| Storage | 1 MiB and 1,024 keys per project; key 128 chars, value 64 KiB |
| Live jobs / job payload | 256 per project / 64 KiB |
| Pending event queue | 1,000 per project; overflow is logged and delivery truncated |
| Match records | 256 observed completed rounds, 512 events per round |
| Compiler/test workers / background execution workers | 2 / 2 |

No filesystem, network/HTTP, process, shell, environment, database handles, eval or user-native extensions are exposed. Rhai growth and instruction limits bound ordinary allocations; there is no OS-enforced heap ceiling or separate interpreter process. The runtime has no compiled AST cache in this version. Owner-only API authorization and per-channel ownership checks guard all editor operations.

The overview returns bounded recent windows (200 reports, 500 jobs/revisions, 2,000 keys, 30 recorded snapshots); server-side pagination of older records is not implemented. The editor's API fields are documented here rather than generated scripting OpenAPI types. Russian localization of the new scripting surfaces is not complete. These are explicit v1 limitations; they do not change trigger/disable/revision safety semantics.

## Developer architecture and deployment

`src/scripting/runtime.rs` owns the sandbox and resolver; `service.rs` owns host operations, quotas and domain calls; `worker.rs` owns durable scheduling, serialization and retention; `matches.rs` consumes existing normalized transitions; `api.rs` owns authorized editor commands. Source ingestion only persists shared snapshots and enqueues work; it never runs Rhai. A short ingestion persistence timeout logs failures without breaking the existing CS2 transport. The companion only adds phase_countdowns to the existing GSI whitelist and recognizes the exact previous owned config during uninstall.

Migrations (apply in order with the existing SQLx startup migration mechanism):

1. 20260928010000_scripting.sql - projects, revisions, storage, jobs, matches, shared snapshots and execution queue.
2. 20260928020000_reward_scripting.sql - visibility and scoped aliases.
3. 20260928030000_fulfillment_origin.sql - separate fulfillment/Twitch identity, origin, attribution and discard state.
4. 20260928040000_script_editor_reports.sql - compile/dry-run report history.
5. 20260928050000_script_audit_attribution.sql - downstream script attribution and no false Twitch-fulfillment audit events.

Deploy backend and frontend together: the internal API identity is now `fulfillment_id`; `twitch_redemption_id` is the actual nullable Twitch ID. Existing UUIDs and all foreign-key identities are preserved. Take a database backup before migration; an old binary is not compatible with the renamed column. No production migration, Twitch mutation or live Market purchase is performed by the automated tests.

Owner API: GET/POST `/api/v1/broadcasters/{channel_id}/scripts`. POST uses an `action` tag: create, rename, enable, delete, save, validate, publish, rollback, test, snapshot, storage_set, storage_delete, storage_clear, run_job, cancel_job. Project actions require project_id. Save/publish require the current version. Delete/clear/run_job require confirmation DELETE/CLEAR/RUN. Snapshot requires snapshot_id and event_index and verifies channel ownership. Viewer discard is POST `/api/v1/me/inventory/{inventory_id}/discard` with `{ "confirmation": "DISCARD" }` and verifies ownership under the inventory lock.

The `examples/` directory is a complete multi-file project covering round streaks, temporary visibility, eligible random chatters, script fulfillment, activity rejection and timer routing. Its compilation and round routing are tested by the Rust suite. Configure reward aliases before enabling it; the giveaway example can trigger on every local kill, subject to independent budgets.

Run backend tests against a fresh disposable TEST_DATABASE_URL with dummy Twitch/App environment variables: `cargo test -- --include-ignored --test-threads=1`. Some existing integration tests assume a fresh database. Frontend: `npm run typecheck`, `npm run lint`, `npm run build`, and `npx playwright test tests/scripts.spec.ts tests/inventory.spec.ts tests/cs2.spec.ts`. Companion: `cargo test` in src-tauri. The credential-manager test stays ignored unless explicitly testing that OS integration.
