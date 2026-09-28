fn handle(ctx) {
    if ctx.timer.key == "hide_reward" {
        rewards.set_visible(ctx.timer.payload.reward, false);
    }
}
