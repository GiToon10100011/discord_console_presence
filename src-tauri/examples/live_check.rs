//! Checks the Rich Presence behaviour against a *real* Discord client.
//!
//! Everything else in the test suite talks to a fake Discord, so it cannot say
//! whether Discord really shows an image or counts the timer correctly. Run
//! this with Discord open, look at your profile, and note what you see:
//!
//! ```text
//! cargo run --example live_check -- <APPLICATION_ID> [options]
//!
//!   --image <KEY|https-URL>   large image: a registered asset key or a public URL
//!   --small <KEY>             small image (asset key)
//!   --ago <MINUTES>           how long ago the timer starts (default 90)
//!   --hold <SECONDS>          how long to keep the presence (default 60, 0 = until Enter)
//!   --raw s|ms                skip the app: send the timestamp as seconds or milliseconds
//! ```
//!
//! A test presence appears on your profile while it runs and is cleared at the end.

use std::{
    env,
    io::BufRead,
    thread,
    time::{Duration, Instant},
};

use console_presence_lib::{
    discord::{build_activity, probe, DiscordHandle, WorkerConfig},
    image::{assets::AssetCatalog, http_client},
    session::PresenceConfig,
    storage::{ImageSource, ImageSpec, Settings},
    util::now_ms,
};
use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use serde_json::json;

struct Options {
    app_id: String,
    image: Option<String>,
    small: Option<String>,
    ago_minutes: i64,
    hold_secs: u64,
    raw: Option<String>,
}

fn parse() -> Option<Options> {
    let mut args = env::args().skip(1);
    let app_id = args.next().filter(|a| !a.starts_with("--"))?;
    let mut options = Options {
        app_id,
        image: None,
        small: None,
        ago_minutes: 90,
        hold_secs: 60,
        raw: None,
    };
    while let Some(flag) = args.next() {
        let value = args.next()?;
        match flag.as_str() {
            "--image" => options.image = Some(value),
            "--small" => options.small = Some(value),
            "--ago" => options.ago_minutes = value.parse().ok()?,
            "--hold" => options.hold_secs = value.parse().ok()?,
            "--raw" if value == "s" || value == "ms" => options.raw = Some(value),
            _ => return None,
        }
    }
    Some(options)
}

fn image_spec(value: &str) -> ImageSpec {
    if value.starts_with("http://") || value.starts_with("https://") {
        ImageSpec {
            source: ImageSource::ExternalUrl,
            url: Some(value.to_string()),
            ..ImageSpec::default()
        }
    } else {
        ImageSpec {
            source: ImageSource::DiscordAsset,
            asset_key: Some(value.to_string()),
            ..ImageSpec::default()
        }
    }
}

fn hold(secs: u64) {
    if secs == 0 {
        println!("\n>>> Look at your Discord profile now. Press Enter to clear it.");
        let _ = std::io::stdin().lock().lines().next();
    } else {
        println!("\n>>> Look at your Discord profile now. Clearing in {secs} s.");
        thread::sleep(Duration::from_secs(secs));
    }
}

/// Sends the timestamp in the unit asked for, bypassing the app's own
/// conversion, to find out which unit the real client expects.
fn raw_timestamp_check(options: &Options, unit: &str, start_ms: i64) {
    let mut client = DiscordIpcClient::new(&options.app_id);
    client.connect_ipc().expect("Discord is not running");
    client
        .send(json!({ "v": 1, "client_id": options.app_id }), 0)
        .expect("handshake");
    let (op, ready) = client.recv().expect("handshake reply");
    println!("handshake: opcode {op}, event {}", ready["evt"]);

    let value = if unit == "ms" { start_ms } else { start_ms / 1000 };
    let mut activity = Activity::new()
        .activity_type(ActivityType::Playing)
        .name("Console Presence timestamp check")
        .details(format!("timestamp sent as: {unit}"))
        .state(format!("timer should read about {} min", options.ago_minutes))
        .timestamps(Timestamps::new().start(value));
    if let Some(image) = &options.image {
        activity = activity.assets(Assets::new().large_image(image.as_str()));
    }
    println!("sending start = {value} ({unit})");
    client.set_activity(activity).expect("set_activity");
    println!("reply: {:?}", client.recv().expect("reply"));

    hold(options.hold_secs);
    let _ = client.clear_activity();
    let _ = client.close();
}

fn main() {
    let Some(options) = parse() else {
        eprintln!("usage: cargo run --example live_check -- <APPLICATION_ID> [--image KEY|URL] [--small KEY] [--ago MIN] [--hold SECS] [--raw s|ms]");
        std::process::exit(2);
    };
    let start_ms = now_ms() - options.ago_minutes * 60_000;

    match probe(&options.app_id) {
        Ok(user) => println!("connected to Discord as {} ({})", user.username, user.id),
        Err(reason) => {
            eprintln!("cannot connect: {reason}");
            std::process::exit(1);
        }
    }

    if let Some(unit) = &options.raw {
        raw_timestamp_check(&options, unit, start_ms);
        return;
    }

    // Same path as the app: asset catalog -> build_activity -> worker.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let http = http_client().expect("http client");
    let catalog = runtime.block_on(AssetCatalog::default().lookup(&http, &options.app_id));
    println!("registered assets: {catalog:?}");

    let settings = Settings {
        application_id: options.app_id.clone(),
        ..Settings::default()
    };
    let config = PresenceConfig {
        game_name: "Console Presence Live Check".into(),
        details: "Live check".into(),
        state: "Verifying image and timer".into(),
        large_image: options.image.as_deref().map(image_spec).unwrap_or_default(),
        large_image_text: "Large image hover text".into(),
        small_image: options.small.as_deref().map(image_spec).unwrap_or_default(),
        small_image_text: "Small image hover text".into(),
        start_timestamp: None,
    };
    let (payload, report) = build_activity(&config, start_ms, &settings, &catalog);
    println!("report: {report:#?}");
    println!(
        "sending: {}",
        serde_json::to_string_pretty(&payload.to_rpc()).unwrap_or_default()
    );

    let handle = DiscordHandle::spawn(WorkerConfig::default(), |status| {
        println!("[status] {:?} {}", status.state, status.message.clone().unwrap_or_default());
    });
    handle.connect(&options.app_id);
    handle.set_activity(Some(payload));

    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline && handle.status().last_presence.is_none() {
        thread::sleep(Duration::from_millis(100));
    }
    match handle.status().last_presence {
        Some(outcome) if outcome.ok => println!("Discord accepted the presence."),
        Some(outcome) => println!("Discord REJECTED the presence: {:?}", outcome.message),
        None => println!("no answer from Discord within 15 s"),
    }

    hold(options.hold_secs);
    handle.set_activity(None);
    thread::sleep(Duration::from_millis(1500));
    handle.shutdown(Duration::from_secs(2));
    println!("presence cleared.");
}
