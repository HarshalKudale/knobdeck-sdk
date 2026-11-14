# Three complete plugins

Each one builds as shown against this SDK version. Start from
`knobdeck-plugin new <folder>` and replace `src/main.rs`.

## 1. A web API with live content: crypto prices

Route: the service's public HTTP API (no key). A live label refreshes every minute.

`Cargo.toml` dependencies:

```toml
knobdeck-sdk = { git = "https://github.com/HarshalKudale/knobdeck-sdk.git" }
ureq = { version = "2", features = ["json"] }
```

`src/main.rs`:

```rust
use knobdeck_sdk::input::{self, InputExt};
use knobdeck_sdk::{Action, Error, Plugin, Result, Value, Visual};
use std::time::Duration;

fn price(coin: &str, currency: &str) -> Result<f64> {
    let reply: Value = ureq::get("https://api.coingecko.com/api/v3/simple/price")
        .query("ids", coin)
        .query("vs_currencies", currency)
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|e| Error::msg(format!("Price service unavailable ({e})")))?
        .into_json()?;
    reply[coin][currency].as_f64().ok_or_else(|| Error::msg(format!("No price for \"{coin}\"")))
}

fn inputs_or_default(coin: String, currency: String) -> (String, String) {
    let coin = if coin.is_empty() { "bitcoin".into() } else { coin.to_lowercase() };
    let currency = if currency.is_empty() { "usd".into() } else { currency.to_lowercase() };
    (coin, currency)
}

fn main() {
    Plugin::new("crypto-prices", "Crypto Prices")
        .version(env!("CARGO_PKG_VERSION"))
        .author("You")
        .description("Live cryptocurrency prices on your keys.")
        .icon("bitcoin")
        .action(
            Action::button("price", "Price")
                .icon("bitcoin")
                .inputs(vec![
                    input::text("coin", "Coin id").placeholder("bitcoin"),
                    input::select("currency", "Currency", &[("usd", "USD"), ("eur", "EUR"), ("inr", "INR")]).default_value("usd"),
                ])
                .live(Duration::from_secs(60), |ic| {
                    let (coin, currency) = inputs_or_default(ic.input_str("coin"), ic.input_str("currency"));
                    let p = price(&coin, &currency)?;
                    Ok(Visual::new().label(format!("{} {p:.0}", currency.to_uppercase())))
                })
                // Pressing opens the coin's page.
                .on_press(|ctx| {
                    let (coin, _) = inputs_or_default(ctx.input_str("coin"), ctx.input_str("currency"));
                    knobdeck_sdk::open_url(&format!("https://www.coingecko.com/en/coins/{coin}"))
                }),
        )
        .run()
}
```

Test: `knobdeck-plugin dev && knobdeck-plugin drive` shows a `state`/visual message with the price
label within a second.

## 2. A local app with a token: Home Assistant

Route: Home Assistant's REST API on the user's network. A setup program collects the URL and a
long-lived token; a query lists entities so users pick from a list instead of typing ids.

`Cargo.toml`:

```toml
[dependencies]
knobdeck-sdk = { git = "https://github.com/HarshalKudale/knobdeck-sdk.git", features = ["secrets"] }
ureq = { version = "2", features = ["json"] }

[[bin]]
name = "home-assistant"
path = "src/main.rs"

[[bin]]
name = "home-assistant-setup"
path = "src/setup.rs"
```

`src/setup.rs` (KnobDeck runs it when the user clicks Setup, with the form's values):

```rust
fn main() {
    knobdeck_sdk::run_setup(|ctx| {
        let url = ctx.input_str("url").trim_end_matches('/').to_string();
        let token = ctx.input_str("token");
        if url.is_empty() || token.is_empty() {
            return ctx.fail("Enter the Home Assistant URL and a long-lived access token.");
        }
        // Check the token before saving it.
        ureq::get(&format!("{url}/api/"))
            .set("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| knobdeck_sdk::Error::msg(format!("Home Assistant didn't accept the token ({e})")))?;
        // The URL is a setting; the token goes to the OS credential store.
        knobdeck_sdk::secrets::set("HA_TOKEN", &token)?;
        knobdeck_sdk::save_env(&[("HA_URL", &url)])
    })
}
```

`src/main.rs`:

```rust
use knobdeck_sdk::input;
use knobdeck_sdk::{env_or, json, Action, Error, Plugin, Result, Value};

fn ha(method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    let (url, token) = (env_or("HA_URL", ""), knobdeck_sdk::secrets::env_or_stored("HA_TOKEN"));
    if url.is_empty() {
        knobdeck_sdk::mark_setup_required("Connect KnobDeck to Home Assistant");
        return Err(Error::msg("Home Assistant isn't set up"));
    }
    let req = ureq::request(method, &format!("{url}{path}")).set("Authorization", &format!("Bearer {token}"));
    let res = match body {
        Some(b) => req.send_json(b),
        None => req.call(),
    };
    match res {
        Ok(r) => Ok(r.into_json().unwrap_or(Value::Null)),
        Err(ureq::Error::Status(401, _)) => {
            knobdeck_sdk::mark_setup_required("The Home Assistant token was rejected");
            Err(Error::msg("Token rejected"))
        }
        Err(e) => Err(Error::msg(format!("Can't reach Home Assistant ({e})"))),
    }
}

fn main() {
    knobdeck_sdk::load_env();
    Plugin::new("home-assistant", "Home Assistant")
        .version(env!("CARGO_PKG_VERSION"))
        .author("You")
        .description("Toggle lights, switches and scenes in Home Assistant.")
        .icon("house")
        .setup("home-assistant-setup", None)
        .setup_inputs(vec![
            input::text("url", "Home Assistant URL"),
            input::text("token", "Long-lived access token"),
        ])
        .query("entities", |_| {
            let states = ha("GET", "/api/states", None)?;
            let items: Vec<Value> = states.as_array().into_iter().flatten()
                .filter_map(|s| {
                    let id = s["entity_id"].as_str()?;
                    let domain = id.split('.').next()?;
                    ["light", "switch", "scene", "script"].contains(&domain).then(|| {
                        let name = s["attributes"]["friendly_name"].as_str().unwrap_or(id);
                        json!({"value": id, "label": name})
                    })
                })
                .collect();
            Ok(Value::Array(items))
        })
        .action(
            Action::toggle("toggle", "Toggle")
                .state("off", "Off", "lightbulb-off")
                .state("on", "On", "lightbulb")
                .inputs(vec![input::query_select("entity", "Entity", "entities", &[])])
                // Show the real state when the key appears (and on every sync).
                .volatile()
                .initial_with(|ic| {
                    let entity = ic.input_str("entity");
                    let s = ha("GET", &format!("/api/states/{entity}"), None).ok()?;
                    Some(if s["state"] == "on" { "on".into() } else { "off".into() })
                })
                .on_press(|ctx| {
                    let entity = ctx.input_str("entity");
                    let domain = entity.split('.').next().unwrap_or("homeassistant");
                    let service = if matches!(domain, "scene" | "script") { "turn_on" } else { "toggle" };
                    ha("POST", &format!("/api/services/{domain}/{service}"), Some(json!({"entity_id": entity})))?;
                    let s = ha("GET", &format!("/api/states/{entity}"), None)?;
                    ctx.set_state(if s["state"] == "on" { "on" } else { "off" });
                    Ok(())
                }),
        )
        .run()
}
```

## 3. A macOS app through AppleScript: Music

Route: Music's AppleScript dictionary through `osascript` (no keystrokes). macOS only.

```rust
use knobdeck_sdk::{Action, Error, Plugin, Result, Visual};
use std::process::Command;
use std::time::Duration;

/// Run AppleScript and return its output. The first run asks the user to allow KnobDeck to
/// control Music (Automation permission).
fn osascript(script: &str) -> Result<String> {
    let out = Command::new("osascript").args(["-e", script]).output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(Error::msg(if err.contains("-1743") {
            "Allow KnobDeck to control Music in System Settings → Privacy & Security → Automation".to_string()
        } else {
            format!("Music didn't respond: {}", err.trim())
        }));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn music(cmd: &str) -> Result {
    osascript(&format!("tell application \"Music\" to {cmd}")).map(|_| ())
}

fn main() {
    Plugin::new("apple-music", "Music")
        .version(env!("CARGO_PKG_VERSION"))
        .author("You")
        .description("Control the Music app.")
        .platforms(&["macos"])
        .icon("music")
        .action(
            Action::toggle("play-pause", "Play / Pause")
                .state("paused", "Paused", "play")
                .state("playing", "Playing", "pause")
                // Keep the key in step when playback changes outside KnobDeck, and show the track.
                .live(Duration::from_secs(2), |_| {
                    let running = osascript("application \"Music\" is running")? == "true";
                    if !running {
                        return Ok(Visual::new().state("paused").label("Music"));
                    }
                    let info = osascript("tell application \"Music\" to if player state is playing then return name of current track")?;
                    let playing = !info.is_empty();
                    Ok(Visual::new().state(if playing { "playing" } else { "paused" }).label(if playing { info } else { "Paused".into() }))
                })
                .on_press(|_| music("playpause")),
        )
        .action(Action::button("next", "Next Track").icon("skip-forward").on_press(|_| music("next track")))
        .action(Action::button("previous", "Previous Track").icon("skip-back").on_press(|_| music("previous track")))
        .action(
            Action::slider("volume", "Volume", 0.0, 100.0).unit("%").step(5.0).on_adjust(|ctx| {
                let now: f64 = osascript("tell application \"Music\" to get sound volume")?.parse().unwrap_or(50.0);
                let v = ctx.value.unwrap_or(now + ctx.delta.unwrap_or(0.0)).clamp(0.0, 100.0);
                music(&format!("set sound volume to {}", v.round()))?;
                ctx.set_state(v.round());
                Ok(())
            }),
        )
        .run()
}
```
