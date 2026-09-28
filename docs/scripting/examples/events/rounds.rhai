fn handle(ctx) {
    if ctx.current_match == () { return; }
    let rounds = ctx.current_match.last_rounds(3);
    // Unknown kills must not be treated as zero or as a qualifying round.
    if rounds.len == 3
        && rounds[0].index != () && rounds[1].index != () && rounds[2].index != ()
        && rounds[1].index == rounds[0].index + 1
        && rounds[2].index == rounds[1].index + 1
        && rounds.all(|r| r.player.kills != () && r.player.kills >= 3) {
        rewards.enable_for("triple_triple", Duration::from_secs(333));
    }
}
