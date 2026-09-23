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
`REJECTED`; a created order becomes `ORDER_CREATED` and may progress to
`TRADE_WAITING`, `DELIVERED`, `SELLER_FAILED` or `BUYER_FAILED`. Market stage 5
provides the terminal failure evidence used for a later explicit attempt.
A watcher timeout or an unsuccessful lookup is never evidence that another
purchase is safe. An explicit retry uses a new unique custom ID for the same
item and ceiling, after locking the inventory row and checking the latest
attempt. A 30-second server cooldown limits repeated explicit requests.
HTTP errors, malformed responses, unknown Market errors and unsuccessful
lookups cannot release an attempt for retry. A response that claims rejection
but still contains an order ID is ambiguous too.

The redemption message is checked for a Steam trade link first; the viewer's
manually saved link is the fallback. A valid message link rejected by Market is
not silently replaced by the saved link. The viewer can explicitly choose the
saved link for a later attempt. A message link is never saved to settings.

Buyer-fault terminal attempts allow a viewer retry only when the snapshotted
reward setting allows it. An operator may act through the channel permission
path. Insufficient Market balance leaves the item and Twitch redemption
pending and can be retried explicitly. `refund_if_chat_req_failed` still
governs chat eligibility before item selection; it creates no inventory item.

## Delivery and refund

Existing order polling can mark delivery at Market stage 2 (`ITEM_GIVEN`),
without waiting for settlement. The inventory item and local redemption are
marked delivered/completed under one row lock. The subsequent Twitch
fulfillment is recorded in `twitch_fulfilled_at`; a missing marker on a
delivered live item is retried by startup and periodic recovery. This can
repeat a Twitch status request if the remote response was lost, but cannot
repeat a Market purchase.

Viewer/operator refund first reconciles the latest Market attempt when needed.
The same inventory lock reserves `REFUNDING` only if every attempt is provably
terminal or rejected and the redemption is pending. Delivery cannot win that
lock afterward. A successful Twitch refund closes the item as `REFUNDED` and
the redemption as `FAILED_REFUND`. An uncertain Twitch refund leaves
`RECONCILIATION_REQUIRED` for review, so it cannot be retried blindly.

## Channel chat

The reward's channel chat receives customizable inventory status messages.
New inventory snapshots announce either viewer action or operator review when
auto-buy is off. A successful Market response announces an order; detecting a
Steam trade announces its offer link; confirmed delivery announces acceptance.
Missing links, definitive rejections, ambiguous Market outcomes, terminal trade
failures, and explicit refunds use separate templates that describe the current
points state. These notices are best effort: chat failure never rolls back a
persisted fulfillment transition. Database transitions suppress repeat notices
from duplicate redemptions, concurrent watchers, and repeated polls.
The channel message settings expose only templates used by the current
fulfillment flow. Obsolete Market refund, penalty, automatic retry, and
filter fallback templates are ignored. No migration deletes historical custom
JSON; a later channel settings save may drop those ignored keys.

## Migration and follow-up tracking

There is no historical inventory backfill: old redemption/paid-price rows do
not reliably contain the initial item and attempted ceiling. Existing
inventory rows from the preceding migration retain their raw data and become
`LEGACY_REVIEW`; in particular, old filter fallback could have stored candidate
B with candidate A's price. The migration makes no external calls.
Already active legacy orders continue through their existing watcher and may
complete Twitch delivery, but legacy rows cannot initiate another order through
the new inventory action paths.

The upcoming tracking task should continue each item's existing order/trade
timeline through seller trade creation, trade ID, stage 1/2/5, send/receive
deadlines, causer, cancellation, Market refund/penalty details and settlement.
Those details belong to the inventory attempt and item; the redemption remains
the Twitch points outcome. This task keeps the existing 30-minute watcher
interval/deadline, representing expiry as reconciliation required rather than
permission to create another order.
