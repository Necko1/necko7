# Scripting documentation and maintenance

The user-facing documentation lives in the sibling dashboard repository, `necko7-frontend/docs/site`, and is built as a static VitePress site at `/docs/scripting/`. Open Scripts > Scripting documentation. It covers all host APIs, data shapes, workflows, access, limits, match uncertainty and eleven complete cookbook projects.

## Sources of truth

- `api.json` in this directory is the versioned host API/context catalog.
- `recipes.json` contains complete cookbook files and executable test scenarios.
- `events.json` contains every semantic CS2 event's field descriptions, observation pair, full-envelope example and native Rhai handler. It generates one reference page with deep-linkable event sections, not one page per event.
- `src/scripting/runtime.rs` implements native Rhai registration and the sandbox.
- `service.rs` implements host operations; `worker.rs` implements dispatch and durable timers.
- The frontend keeps versioned catalog snapshots for independent builds. Its Monaco completion/hover provider consumes that snapshot, not a second handwritten signature list.

After changing APIs, events or recipes, run `npm run api:sync` in the dashboard repository. This updates the versioned snapshots and generated reference/cookbook pages. `npm run docs:check` rejects stale generated content and cross-repository catalog drift when the backend checkout is available. The backend documentation tests compare the catalog against the engine's actual registered names and overload arities, verify normalized schema keys, execute every API example, and run every cookbook scenario. Event tests run the documented observation pairs through the real normalizer and compare the complete serialized payloads, with an exhaustive EventKind match requiring coverage for new variants. Every event handler executes with matching, different-kind and nullable-field inputs. This is bounded metadata generation, not runtime code generation; prose still requires review when derivation behavior changes.

The static site's handwritten guides explain behavior not captured by signatures: revision pinning, side effects, dry-run limitations, Owner/Editor security, project-only imports, unknown data and safe failure handling. Update those explanations when behavior changes. See `necko7-frontend/docs/README.md` for build/deployment and link checks.

## Access and compatibility

Owner and Editor can operate normal Scripts. Creating/replacing pairing codes and dashboard device revocation remain Owner-only. A native desktop claims an Owner-issued one-shot code and uses its Ed25519 signature for device requests; an authenticated Editor session cannot claim or unpair a device through those endpoints. Viewer/unrelated users have no operator workspace.

Rhai is pinned to 1.26.1. Use native `ctx.current_match`, `UserFilter::create()` and `RewardFilter::create()`. There is no syntax rewriting. Rhai reserves `debug` in method position: use the working native `debug(log, "message")` operation, not `log.debug(...)`. Other report levels use `log.info/warn/error`.

`users.recent_chatters(window, filter)` (alias `chat.recent_chatters`) uses its window only to select candidate channel chatters. `UserFilter.activity(ActivityFilter::create()...)`, `.messages(MessageFilter::any()/all()...)` and `.reward_redemptions(RewardFilter::create()...)` are ANDed; each has its own optional `.during(Duration)`. Omitted means all retained channel history up to query time, never implicit reuse of the candidate window. Explicit ActivityFilter/MessageFilter windows accept 1 second..365 days; RewardFilter/candidate/user_stats windows retain a 60-second minimum. Returned user activity counts/timestamps always describe the candidate window. Released UserFilter.min_messages/min_characters remain compatibility conveniences using that original candidate window; prefer ActivityFilter for new scripts. See the public user-filtering guide for migration and all-history caveats.

MessageFilter `all()` requires one authored message satisfying every clause, not separate messages per clause. Builders accept 1..16 clauses, each with 1..256 Unicode scalar values and no NUL. Patterns are literal, not regex or SQL wildcards. Case sensitivity defaults to false and applies to every clause. Insensitive SQL uses the standard deterministic ICU `und-x-icu` collation for Unicode/Cyrillic regardless of the default database locale; a custom PostgreSQL build must include ICU and this collation. The project's `postgres:18-alpine3.22` image supplies it. Sensitive SQL uses `C` collation. No Unicode normalization or accent removal is performed. Activity thresholds count all messages in their own activity window, not just text matches. SQL materializes eligibility ID sets once and applies the 1,000-user result limit after filtering; raw history never enters Rhai.

Live event context contains state/previous/current_match. Live timer context contains timer/source/meta/actor_type only: carry needed JSON-safe evidence in the payload. Hidden rewards use a configured script_alias and is_visible=false while remaining operationally unpaused. Trigger budgets count calls per execution and persisted fulfillment rows per project/minute, not all rejected attempts.

`rewards.trigger` leaves the reward/selection/inventory/waiting-mode and successful Market-order announcement to script code (`chat.send`). Pre-inventory admission errors remain quiet. After inventory admission, the normal fulfillment pipeline sends actionable missing-trade-link, purchase-error/reconciliation and Steam-trade notices, including acceptance links/deadlines, acceptance confirmation and terminal delivery failures. A script may pass a third argument with selected buyer/pre-order notice keys (an array or one string) to replace those notices with its own chat message after inspecting the result: `rewards.trigger("secret_case", user_id, ["trade_link_required", "unavailable"])`. This choice is stored per fulfillment and also applies to later authorized purchase attempts. Reconciliation and order/trade tracking notices cannot be suppressed. An invalid or non-suppressible key causes a script error before the trigger is processed. If the script's replacement `chat.send` fails, the suppressed standard notice is not sent later. The public scripting API reference and custom-notice cookbook recipe document the full allowlist and example. This applies to initial auto-buy, authorized manual attempts and background tracking. All origins use the same channel-configured delivery templates and shared neutral defaults, without SCRIPT-specific text substitution; Twitch points/refund notices retain separate Twitch-only keys. Channel-authored delivery templates are honored. A viewer can save a missing trade link and start delivery of the existing AUTO/VIEWER item without retriggering; OPERATOR items still require an operator. Saving the link alone does not initiate a purchase. No Twitch redemption/status/refund operation is introduced for SCRIPT.

## Development checks

Against a fresh disposable PostgreSQL database with dummy Twitch/App configuration:

```powershell
cargo test --locked -- --include-ignored --test-threads=1
```

The ignored HTTP access regression requires TEST_DATABASE_URL. It exercises Owner/Editor/non-operator access to Scripts and device authority using real handlers and persistence.

The message-filter PostgreSQL regressions require only a disposable TEST_DATABASE_URL (no Twitch credentials). Run `cargo test --locked scripting::message_filter_tests -- --include-ignored --test-threads=1` to exercise the real query path, Unicode/case modes, same-message AND, literal SQL characters, time/channel boundaries and native Rhai builder serialization. Existing migrations are applied; these tests must not use a production database.

Run `cargo test --locked scripting::filter_window_tests -- --include-ignored --test-threads=1` for independent candidate/activity/content/reward windows, optional all-history behavior, compatibility thresholds, validation and filtering-before-limit coverage.

In the dashboard:

```text
npm run api:sync
npm run docs:check
npm run typecheck
npm run lint
npm run build
npm test
```

Build includes the static docs and checks internal links/anchors. Browser tests cover docs navigation/search/narrow views, pairing, Editor access and file context menus. The older `examples/` project remains a tested regression fixture; current user recipes are generated from recipes.json. Historical implementation reports describe their original pass, not the current access/API contract.

No production database migration, Twitch mutation, Market purchase or deployment is performed by these documentation checks.
