import "events/rounds" as rounds;
import "events/giveaway" as giveaway;
import "timers/router" as timers;

fn on_event(ctx) {
    switch ctx.event.kind {
        "round_ended" => rounds::handle(ctx),
        "player_kill" => giveaway::handle(ctx),
        _ => ()
    }
}
fn on_timer(ctx) { timers::handle(ctx); }
