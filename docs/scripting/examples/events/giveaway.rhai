fn handle(ctx) {
    let filter = UserFilter::create().min_messages(5).min_characters(200);
    let chatters = chat.recent_chatters(Duration::from_mins(30), filter);
    let winner = random.pick(chatters);
    if winner == () { return; }
    let result = rewards.trigger("random_skin", winner.id);
    if result.ok {
        log.info(`Fulfillment admitted for ${winner.login}`);
    } else if result.code == "activity_requirement_failed" {
        log.info("Selected user does not meet this reward's chat requirement");
    } else {
        log.warn(`Trigger rejected: ${result.code}`);
    }
    // This example intentionally sends no automatic results to Twitch chat.
}
