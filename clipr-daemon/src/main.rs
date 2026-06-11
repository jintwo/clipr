use anyhow::Result;
use clap::Parser;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::{NSInteger, NSString};
use rustyline::DefaultEditor;
use std::fs::File;
use std::io::prelude::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
mod http;

static USAGE: &str = include_str!("usage.txt");
static DEFAULT_LIFETIME: Duration = Duration::from_secs(60 * 60 * 24 * 7 * 2);

fn get_change_count() -> NSInteger {
    NSPasteboard::generalPasteboard().changeCount()
}

fn get_current_entry() -> Option<String> {
    unsafe {
        if let Some(value) = NSPasteboard::generalPasteboard().stringForType(NSPasteboardTypeString)
        {
            let bytes = value.UTF8String() as *const u8;
            let length = value.len();
            let string = std::str::from_utf8(std::slice::from_raw_parts(bytes, length)).unwrap();
            Some(String::from(string))
        } else {
            None
        }
    }
}

fn set_current_entry(s: String) {
    unsafe {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();

        let value = NSString::from_str(&s);
        pb.setString_forType(&value, NSPasteboardTypeString);
    }
}

fn clipboard_sync(sender: Sender<clipr_common::Request>) {
    let mut last_hash: u64 = 0;
    let mut last_change_count: isize = 0;
    loop {
        thread::sleep(Duration::from_millis(500));
        let change_count = get_change_count();
        if last_change_count == change_count {
            continue;
        } else {
            last_change_count = change_count;
        }
        match get_current_entry() {
            None => continue,
            Some(val) => {
                let hash = clipr_common::calculate_hash(&val);
                if last_hash == hash {
                    continue;
                }

                last_hash = hash;
                sender.send(clipr_common::Request::Sync(val)).unwrap();
            }
        }
    }
}

fn collect_garbage(duration: Duration, sender: Sender<clipr_common::Request>) {
    loop {
        thread::sleep(Duration::from_secs(1));
        sender
            .send(clipr_common::Request::Cleanup(duration))
            .unwrap();
    }
}

fn cmd_line_loop(sender: Sender<clipr_common::Request>) {
    let mut rl = DefaultEditor::new().unwrap();
    loop {
        let readline = rl.readline(":> ");
        match readline {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(line.as_str());

                let mut cmd_line = shellwords::split(line.as_str()).unwrap();
                let bin_name = std::env::args().next().unwrap();
                cmd_line.insert(0, bin_name);

                let cmd = match clipr_common::Args::try_parse_from(cmd_line) {
                    Ok(args) => args.command.unwrap(),
                    Err(_) => clipr_common::Command::Help,
                };

                match clipr_common::Request::send_cmd(&sender, cmd) {
                    Some(clipr_common::Response::Stop) => return,
                    Some(clipr_common::Response::Payload(val)) => {
                        println!("{}", String::from(&val))
                    }
                    _ => continue,
                }
            }
            Err(_) => sender.send(clipr_common::Request::Quit).unwrap(),
        }
    }
}

fn empty_fg_loop(sender: Sender<clipr_common::Request>) {
    let mut rl = DefaultEditor::new().unwrap();
    loop {
        let readline = rl.readline("");
        match readline {
            Ok(_) => continue,
            Err(_) => {
                sender.send(clipr_common::Request::Quit).unwrap();
            }
        }
    }
}

fn event_loop(state: Arc<clipr_common::State>, receiver: Receiver<clipr_common::Request>) {
    let s = state.clone();
    loop {
        if let Ok(msg) = receiver.recv() {
            match msg {
                clipr_common::Request::Quit => return,
                clipr_common::Request::Cleanup(value) => {
                    let mut entries = s.entries.lock().unwrap();
                    loop {
                        if !entries
                            .delete_one_older_than(value, s.config.min_entries.unwrap_or(512))
                        {
                            break;
                        }
                    }
                }
                clipr_common::Request::Sync(value) => {
                    let mut entries = s.entries.lock().unwrap();
                    entries.insert(value)
                }
                clipr_common::Request::Command(cmd, sender) => {
                    let payload = handle_call(s.clone(), cmd).unwrap();
                    match payload {
                        clipr_common::Payload::Stop => return,
                        _ => {
                            sender
                                .send(clipr_common::Response::Payload(payload))
                                .unwrap();
                            continue;
                        }
                    }
                }
            };
        }
    }
}

fn save_db(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = File::create(db_path)?;
    let data = serde_json::to_string_pretty(&state.entries)?;
    file.write_all(data.as_bytes())?;
    Ok(())
}

fn save_db_sync(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = File::create(db_path)?;
    let data = serde_json::to_string_pretty(&state.entries)?;
    file.write_all(data.as_bytes())?;
    Ok(())
}

fn load_db(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = File::open(db_path)?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;
    let data: clipr_common::Entries = serde_json::from_str(buffer.as_str())?;
    let mut entries = state.entries.lock().unwrap();
    *entries = data;
    drop(entries);
    Ok(())
}

// TODO: use state.handle_call + Mutex around State
fn handle_call(
    state: Arc<clipr_common::State>,
    cmd: clipr_common::Command,
) -> Result<clipr_common::Payload> {
    Ok(match cmd {
        clipr_common::Command::List {
            from_index,
            to_index,
            preview_length,
        } => {
            let entries = state.entries.lock().unwrap();
            let items = entries.select_by_range(from_index, to_index);
            clipr_common::Payload::List {
                value: items,
                preview_length,
            }
        }
        clipr_common::Command::Count => {
            let entries = state.entries.lock().unwrap();
            clipr_common::Payload::Value {
                value: Some(entries.len().to_string()),
            }
        }
        clipr_common::Command::Save => {
            save_db(state.clone()).unwrap();
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Load => {
            load_db(state.clone()).unwrap();
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Get { index } => {
            let mut entries = state.entries.lock().unwrap();
            match entries.get_value(index) {
                Some(val) => clipr_common::Payload::Value { value: Some(val) },
                None => clipr_common::Payload::Message {
                    value: format!("item at {index:?} not found"),
                },
            }
        }
        clipr_common::Command::Add { value } => {
            set_current_entry(value.join(" "));
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Insert { filename } => {
            let mut file = File::open(filename)?;
            let mut buffer = String::new();
            file.read_to_string(&mut buffer)?;
            set_current_entry(buffer);
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Set { index } => {
            let mut entries = state.entries.lock().unwrap();
            if let Some(value) = entries.get_value(index) {
                set_current_entry(value);
                clipr_common::Payload::Ok
            } else {
                clipr_common::Payload::Message {
                    value: format!("item at {index:?} not found"),
                }
            }
        }
        clipr_common::Command::Del {
            from_index,
            to_index,
        } => {
            let mut entries = state.entries.lock().unwrap();
            entries.delete(from_index, to_index);
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Tag { index, tag } => {
            let mut entries = state.entries.lock().unwrap();
            if entries.tag(index, tag) {
                clipr_common::Payload::Ok
            } else {
                clipr_common::Payload::Message {
                    value: format!("item at {index:?} not found"),
                }
            }
        }
        clipr_common::Command::Untag { index, tag } => {
            let mut entries = state.entries.lock().unwrap();
            if entries.untag(index, tag) {
                clipr_common::Payload::Ok
            } else {
                clipr_common::Payload::Message {
                    value: format!("item at {index:?} not found"),
                }
            }
        }
        clipr_common::Command::Pin { index, pin } => {
            let mut entries = state.entries.lock().unwrap();
            entries.pin(index, pin.to_uppercase().next().unwrap());
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Unpin { index } => {
            let mut entries = state.entries.lock().unwrap();
            entries.unpin(index);
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Select {
            set,
            pin,
            tag,
            value,
        } => {
            let entries = state.entries.lock().unwrap();

            if pin.is_none() && tag.is_empty() && value.is_none() {
                return Ok(clipr_common::Payload::Message {
                    value: String::from("invalid args"),
                });
            };

            let items: Vec<(usize, clipr_common::Item)> = entries.select(
                pin.map(|s| s.to_uppercase().chars().next().unwrap()),
                tag,
                value,
            );

            if set && !items.is_empty() {
                let (_, item) = &items[0];
                set_current_entry(item.value.clone());
                clipr_common::Payload::Ok
            } else {
                clipr_common::Payload::List {
                    value: items,
                    preview_length: None,
                }
            }
        }
        clipr_common::Command::Tags => {
            let entries = state.entries.lock().unwrap();
            let tags = entries.get_tags();
            let mut ts = tags.into_iter().collect::<Vec<String>>();
            ts.sort();
            clipr_common::Payload::Value {
                value: Some(ts.join(":")),
            }
        }

        clipr_common::Command::Help => clipr_common::Payload::Message {
            value: USAGE.to_string(),
        },
        clipr_common::Command::Quit => clipr_common::Payload::Stop,
    })
}

fn parse_lifetime(s: String) -> Duration {
    let value = s
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse::<u32>()
        .unwrap_or(1);
    let unit = s.chars().find(|c| c.is_alphabetic()).unwrap_or('w');
    let mul: u32 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 60 * 60,
        'd' => 60 * 60 * 24,
        'w' => 60 * 60 * 24 * 7,
        _ => 60 * 60 * 24 * 7,
    };

    Duration::from_secs((value * mul).into())
}

fn main() -> Result<()> {
    env_logger::init();
    let args = clipr_common::Args::parse();
    let config = clipr_common::Config::load_from_args(&args)?;
    let state = Arc::new(clipr_common::State::new(config));
    // load db at start
    load_db(state.clone())?;

    let (sender, receiver) = channel::<clipr_common::Request>();
    {
        let sender = sender.clone();
        thread::spawn(move || clipboard_sync(sender));
    }
    {
        let sender = sender.clone();
        let duration = state
            .config
            .lifetime
            .clone()
            .map(parse_lifetime)
            .unwrap_or(DEFAULT_LIFETIME);
        thread::spawn(move || collect_garbage(duration, sender));
    }
    {
        let sender = sender.clone();
        let state = state.clone();
        thread::spawn(move || loop {
            let result = http::server(state.config.listen_on(), sender.clone());
            println!("server died with {result:?}");
        });
    }
    {
        let sender = sender.clone();
        if !state.config.interactive.unwrap_or(false) {
            thread::spawn(move || empty_fg_loop(sender));
        } else {
            thread::spawn(move || cmd_line_loop(sender));
        }
    }

    event_loop(state.clone(), receiver);

    // sync state at exit
    save_db_sync(state)?;
    Ok(())
}
