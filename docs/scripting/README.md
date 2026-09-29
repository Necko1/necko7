# Scripting documentation and maintenance

The user-facing documentation lives in the sibling dashboard repository, `necko7-frontend/docs/site`, and is built as a static VitePress site at `/docs/scripting/`. Open Scripts > Scripting documentation. It covers all host APIs, data shapes, workflows, access, limits, match uncertainty and ten complete cookbook projects.

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

Live event context contains state/previous/current_match. Live timer context contains timer/source/meta/actor_type only: carry needed JSON-safe evidence in the payload. Hidden rewards use a configured script_alias and is_visible=false while remaining operationally unpaused. Trigger budgets count calls per execution and persisted fulfillment rows per project/minute, not all rejected attempts.

## Development checks

Against a fresh disposable PostgreSQL database with dummy Twitch/App configuration:

```powershell
cargo test --locked -- --include-ignored --test-threads=1
```

The ignored HTTP access regression requires TEST_DATABASE_URL. It exercises Owner/Editor/non-operator access to Scripts and device authority using real handlers and persistence.

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
