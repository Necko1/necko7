# Viewer inventory lifecycle

`inventory_items` holds one concrete item per Twitch redemption. Its owner is
the redemption's stable Twitch `user_id`, available from EventSub even before
the viewer logs in. The unique `redemption_id` and database trigger protect
ownership, redemption, item name, fixed price, currency, fulfillment mode and
buyer retry policy. Viewer queries use the authenticated Twitch ID and can
filter this one global inventory by channel, lifecycle status and item name.
Operators use the existing channel permission guard. Channel and reward
provenance remains on the redemption and reward.

## Price and selection

Fixed, pool and filter rewards select exactly one concrete item. The inventory
`fixed_price` is the selected item's first Market `/buy-for` `max_price` ceiling
in the reward currency's minor units. It includes the configured deviation and
the filter cap where applicable. It is not the refreshable reward reference
price, the Market paid price, the Twitch points cost or a refund amount. The
item name and ceiling are inserted together and committed before `buy-for`.
All later attempts use this same item and fixed ceiling; each attempt also
stores its own `item_name` and `max_price`. Neither reward price refresh nor
Market rejection changes the inventory snapshot. There is no automatic
candidate fallback or automatic new order after any Market failure.

## Fulfillment decisions

The reward's `market_autobuy` and the viewer's `viewer_settings.auto_buy_enabled`
are read at inventory creation. Viewer auto-buy defaults to **on** to preserve
the previous automatic flow. `AUTO` makes one initial order; `VIEWER` waits for
an explicit viewer action; `OPERATOR` waits for operator review. These decisions
and the reward's `retry_on_buyer_failure` value are snapshotted on the item.
Changing settings later does not restart an existing item. Legacy inventory
from the preceding migration is `LEGACY_REVIEW` and cannot create an order via
the new action paths.
If a worker stops after persisting a new redemption but before creating its
item, a stale resolution claim is retried. A concrete name already saved on
the redemption is retained; if its reward configuration no longer supports
that name, the redemption stays pending for review instead of switching items.

An attempt row is claimed under an inventory row lock before the external call.
`CALLING` and transport errors remain ambiguous until Market lookup by the
same `custom_id` reconciles them. A known unsuccessful buy-for response becomes
`REJECTED`; a created order becomes `ORDER_CREATED`, then may progress through
`TRADE_WAITING` and `TRADE_ACCEPTED`. `TRADE_WAITING` requires both a positive
`receive_until` and a `trade_id` in one Market observation. `TRADE_ACCEPTED`
records settlement while stage 1 is still active; it does not deliver the item.
Stage 2 alone produces `DELIVERED`. Stage 5 produces `SELLER_FAILED`,
`BUYER_FAILED` or `TERMINAL_UNCLASSIFIED`. A local deadline or an unsuccessful
lookup is never evidence that another purchase is safe. An explicit retry uses
a new unique custom ID for the same
item and ceiling, after locking the inventory row and checking the latest
attempt. A 30-second server cooldown limits repeated explicit requests.
HTTP errors, malformed responses, unknown Market errors and unsuccessful
lookups cannot release an attempt for retry. A response that claims rejection
but still contains an order ID is ambiguous too.

The redemption message is checked for a Steam trade link first; the viewer's
manually saved link is the fallback. A valid message link rejected by Market is
not silently replaced by the saved link. The viewer can explicitly choose the
saved link for a later attempt. A message link is never saved to settings.

Buyer-not-accepted terminal attempts allow a viewer retry only when the snapshotted
reward setting allows it. If the buyer reverted an accepted trade, the viewer
cannot retry or self-refund regardless of that setting. An operator may act through the channel permission
path. Insufficient Market balance leaves the item and Twitch redemption
pending and can be retried explicitly. `refund_if_chat_req_failed` still
governs chat eligibility before item selection; it creates no inventory item.

## Delivery and refund

Market stage 2 is the sole final delivery signal. Settlement is saved as
acceptance evidence but cannot complete inventory or Twitch fulfillment.
The inventory item and local redemption are
marked delivered/completed under one row lock. The subsequent Twitch
fulfillment is recorded in `twitch_fulfilled_at`; a short
`twitch_fulfillment_claimed_at` lease prevents concurrent status writes, and a
missing fulfilled marker on a delivered live item is retried by periodic
recovery. This can
repeat a Twitch status request if the remote response was lost, but cannot
repeat a Market purchase.

Viewer/operator refund first reconciles the latest Market attempt when needed.
The same inventory lock reserves `REFUNDING` only if every attempt is provably
terminal or rejected and the redemption is pending. Delivery cannot win that
lock afterward. A viewer cannot reserve a self-refund if any attempt for the
item ended as `buyer_reverted`, including after a later operator-initiated
attempt ends in a seller failure. Operator refund retains its existing channel
permission.
A successful Twitch refund closes the item as `REFUNDED` and
the redemption as `FAILED_REFUND`. An uncertain Twitch refund leaves
`RECONCILIATION_REQUIRED` for review, so it cannot be retried blindly.

## Channel chat

The reward's channel chat receives customizable inventory status messages.
New inventory snapshots announce either viewer action or operator review when
auto-buy is off. A successful Market response announces an order; detecting a
Steam trade announces its offer link; observed settlement announces acceptance
while explicitly stating that Market has not yet confirmed the final outcome.
Missing links, definitive rejections, ambiguous Market outcomes, terminal trade
failures, and explicit refunds use separate templates that describe the current
points state. These notices are best effort: chat failure never rolls back a
persisted fulfillment transition. Database transitions suppress repeat notices
from duplicate redemptions, concurrent watchers, and repeated polls.
Market rejection codes use distinct `market_errors` templates, without claiming
an automatic points refund. Exact copies of the old Market defaults saved as
channel overrides are ignored so their obsolete refund text is not sent;
separately customized overrides remain intact. Obsolete automatic refund,
penalty, retry, and filter fallback templates are ignored. No migration deletes
historical custom JSON; a later channel settings save may drop ignored keys.
The item-unavailable notice includes the inventory's fixed purchase ceiling in
major currency units (two decimals for RUB, three for USD/EUR). An uncertain
order or trade notice says only that the status is unconfirmed and actions are
unavailable. Stage-5 seller and buyer reverts after observed settlement have
separate configurable notices. Attempt milestone claims prevent duplicate chat
messages from concurrent polling or restart; chat remains best effort.
An exact copied old `trades.accepted` default that said to enjoy the skin is
ignored, so settlement cannot accidentally announce final delivery; separately
customized channel wording remains intact.

## Durable Market tracking and migration

There is no historical inventory backfill: old redemption/paid-price rows do
not reliably contain the initial item and attempted ceiling. Existing
inventory rows from the preceding migration retain their raw data and become
`LEGACY_REVIEW`; in particular, old filter fallback could have stored candidate
B with candidate A's price. The migration makes no external calls.
Live attempts are selected from the database with a short poll lease. GIBCI is
queried for the same `custom_id` about once per minute during the first 30
minutes, then about once per five minutes indefinitely until Market returns
stage 2 or 5. Restart resumes these rows; it does not issue a new buy-for call.
`send_until`, `receive_until`, `trade_id`, settlement, Market stage, causer,
cancellation reason and raw Market refund data are stored on the attempt.
Non-null history survives stage-5 responses that clear fields. Stage 5 without
observed trade creation is seller-not-sent only for new attempts with complete
observation history. Old attempts have `evidence_complete = false` and missing
history stays unclassified. A known stage 5 is terminal even when its fault
category needs operator review. For a complete new attempt, stage 5 without a
trade is `seller_not_sent`; an unaccepted trade uses `buyer_not_accepted` or
`seller_cancelled` according to `causer`; observed settlement uses
`buyer_reverted` or `seller_reverted`. Only `buyer_not_accepted` uses the
snapshotted viewer retry permission. Once any attempt ends as `buyer_reverted`,
viewer self-service retry and refund stay blocked for that inventory item;
subsequent attempts require operator action. Otherwise seller categories allow
an explicit retry for the same item and fixed ceiling. Unknown or contradictory evidence becomes
`OPERATOR_REVIEW` without a viewer retry. A confirmed stage 5 can permit an
explicit points refund because that attempt can no longer deliver, except
that viewers cannot self-refund if any attempt for the item had their own
post-acceptance revert.

The previous watcher could mark an attempt delivered when settlement appeared
at stage 1. The migration reopens those live inventory attempts with their
existing `custom_id` for GIBCI verification. Stage 2 restores confirmed
delivery; stage 5 sends an already completed Twitch redemption to operator
review without another purchase or points refund. The migration also corrects
old seller-failure rows whose `causer` was missing or not seller, because the
old boolean classification treated every non-buyer value as seller. No Market
order or Twitch status write occurs during migration.

Already active legacy orders continue through the compatibility watcher and
may complete Twitch delivery, but legacy rows cannot initiate another order
through the new inventory action paths. The legacy watcher also has no local
terminal timeout. Legacy orders without a trusted attempt do not gain invented
historical trade evidence or Market refund records. Future work can add a full
attempt timeline UI and richer Market-side settlement accounting; neither
changes the Twitch points outcome or immutable inventory value.
