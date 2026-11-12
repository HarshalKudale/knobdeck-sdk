//! Port of the JS persistent example: button, toggle, hold, slider, query-backed
//! inputs and a background ticker. Run `knobdeck-plugin dev` from a crate using it.

use knobdeck_sdk::input::{self, InputExt};
use knobdeck_sdk::{json, Action, Plugin, Visual};
use std::time::Duration;

fn main() {
    Plugin::new("example-rust", "Example Rust")
        .version("1.0.0")
        .author("KnobDeck")
        .description("Reference plugin in Rust.")
        .action(
            Action::button("ping", "Ping (log)")
                .inputs(vec![input::text("message", "Message").placeholder("hello")])
                .on_press(|ctx| {
                    ctx.log(format!("ping instance={} inputs={}", ctx.instance_id, ctx.inputs));
                    println!("stray stdout write"); // lands in the log, not the protocol stream
                    Ok(())
                }),
        )
        .action(
            Action::toggle("mute", "Demo Mute (fan-out)")
                .state("unmuted", "Unmuted", "🔊")
                .state("muted", "Muted", "🔇")
                .on_press(|ctx| {
                    ctx.fan_out(if ctx.state.as_deref() == Some("muted") { "unmuted" } else { "muted" });
                    Ok(())
                }),
        )
        .action(
            Action::hold("ptt", "Push-to-talk")
                .released("Idle", "○")
                .held("Holding", "●")
                .on_hold_start(|ctx| Ok(ctx.set_state("held")))
                .on_hold_end(|ctx| Ok(ctx.set_state("released"))),
        )
        .action(
            Action::slider("volume", "Volume", 0.0, 100.0).unit("%").initial("50").on_adjust(|ctx| {
                let v = ctx.value.unwrap_or(50.0 + ctx.delta.unwrap_or(0.0)).clamp(0.0, 100.0);
                ctx.set_state(v);
                Ok(())
            }),
        )
        .action(
            // Targeted fan-out: sliders on the same device follow each other; the
            // step size is a per-instance setting, so it doesn't decide who syncs.
            Action::slider("device-volume", "Device Volume", 0.0, 100.0)
                .inputs(vec![input::text("device", "Device"), input::text("step", "Step")])
                .on_adjust(|ctx| {
                    ctx.fan_out_by(&["device"], ctx.value.unwrap_or(0.0));
                    Ok(())
                }),
        )
        .action(
            Action::button("pick", "Query demo").inputs(vec![
                input::query_select("group", "Group", "groups", &[]),
                input::query_select("item", "Item", "items", &["group"]),
            ]).on_press(|ctx| ctx.fail("demo error")),
        )
        .action(
            Action::button("expire", "Expire token (setup demo)")
                .on_press(|ctx| Ok(ctx.mark_setup_required("demo token expired"))),
        )
        .query("groups", |_| Ok(json!([{"value": "fruit", "label": "Fruit"}, {"value": "veg", "label": "Veg"}])))
        .query("items", |ctx| {
            Ok(match ctx.input_str("group").as_str() {
                "fruit" => json!(["apple", "pear"]),
                _ => json!(["carrot", "pea"]),
            })
        })
        .on_ready(|rt| {
            let rt = rt.clone();
            std::thread::spawn(move || {
                let mut n = 0;
                loop {
                    n += 1;
                    rt.notify_visual("ping", Visual::new().label(format!("tick {n}")));
                    std::thread::sleep(Duration::from_secs(3));
                }
            });
            Ok(())
        })
        .on_shutdown(|rt| {
            let _ = std::fs::write(rt.data_dir().join("shutdown.txt"), "bye");
        })
        .run()
}
