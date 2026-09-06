-- Migrate flat chat_messages JSONB structure to 5 nested categories:
-- orders, market_errors, trades, chat_requirements, limits

UPDATE broadcaster_settings
SET chat_messages = jsonb_build_object(
    'orders', jsonb_strip_nulls(jsonb_build_object(
        'created', chat_messages->'order_created',
        'failed', chat_messages->'order_failed',
        'failed_no_money_refund', chat_messages->'order_failed_no_money_refund',
        'failed_no_money_penalty', chat_messages->'order_failed_no_money_penalty',
        'failed_filter_exhausted', chat_messages->'order_failed_filter_exhausted',
        'market_error', chat_messages->'market_error',
        'trade_link_invalid', chat_messages->'trade_link_invalid'
    )),
    'market_errors', '{}'::jsonb,
    'trades', jsonb_strip_nulls(jsonb_build_object(
        'created', chat_messages->'trade_created',
        'accepted', chat_messages->'trade_accepted',
        'failed_buyer_refund', chat_messages->'trade_failed_buyer_refund',
        'failed_buyer_penalty', chat_messages->'trade_failed_buyer_penalty',
        'failed_seller_refund', chat_messages->'trade_failed_seller_refund',
        'timeout', chat_messages->'trade_timeout'
    )),
    'chat_requirements', jsonb_strip_nulls(jsonb_build_object(
        'messages_refund', COALESCE(chat_messages->'chat_req_failed_messages_refund', chat_messages->'chat_req_failed_messages'),
        'messages_penalty', COALESCE(chat_messages->'chat_req_failed_messages_penalty', chat_messages->'chat_req_failed_messages'),
        'characters_refund', COALESCE(chat_messages->'chat_req_failed_characters_refund', chat_messages->'chat_req_failed_characters'),
        'characters_penalty', COALESCE(chat_messages->'chat_req_failed_characters_penalty', chat_messages->'chat_req_failed_characters'),
        'both_refund', COALESCE(chat_messages->'chat_req_failed_both_refund', chat_messages->'chat_req_failed_both'),
        'both_penalty', COALESCE(chat_messages->'chat_req_failed_both_penalty', chat_messages->'chat_req_failed_both')
    )),
    'limits', jsonb_strip_nulls(jsonb_build_object(
        'user_limit_reached', chat_messages->'user_purchase_limit_reached',
        'global_limit_reached', chat_messages->'global_purchase_limit_reached'
    ))
)
WHERE chat_messages IS NOT NULL
  AND chat_messages != '{}'::jsonb
  AND NOT (chat_messages ? 'orders');
