//! The parity plugin: the same plugin as `typescript/sdk/examples/parity.ts`. `tests/parity.rs`
//! drives both through one host script and requires identical frames, so the Rust and TypeScript
//! SDKs can't drift.

use knobdeck_sdk::{json, Action, Plugin};

fn main() {
    Plugin::new("parity", "Parity")
        .version("1.0.0")
        .author("KnobDeck")
        .action(Action::button("ping", "Ping").on_press(|ctx| {
            ctx.set_label("pong");
            Ok(())
        }))
        .action(
            Action::toggle("mute", "Mute")
                .state("unmuted", "Unmuted", "mic")
                .state("muted", "Muted", "mic-off")
                .on_press(|ctx| {
                    ctx.fan_out(if ctx.state.as_deref() == Some("muted") { "unmuted" } else { "muted" });
                    Ok(())
                }),
        )
        .action(
            Action::hold("ptt", "Push to talk")
                .released("Released", "mic-off")
                .held("Talking", "mic")
                .on_hold_start(|ctx| {
                    ctx.set_state("held");
                    Ok(())
                })
                .on_hold_end(|ctx| {
                    ctx.set_state("released");
                    Ok(())
                }),
        )
        .action(Action::slider("volume", "Volume", 0.0, 100.0).unit("%").on_adjust(|ctx| {
            ctx.set_state(ctx.value.unwrap_or(0.0));
            Ok(())
        }))
        .action(Action::button("fail", "Fail").on_press(|ctx| ctx.fail("nope")))
        .action(Action::button("toast", "Toast").on_press(|ctx| ctx.toast("hi")))
        .query("items", |q| Ok(json!(["a", q.input_str("group")])))
        .run()
}
