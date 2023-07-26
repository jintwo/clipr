use anyhow::Result;
use async_std::channel::{bounded, Receiver, Sender};
use async_std::fs::File;
use async_std::prelude::*;
use async_std::task;
use clap::Parser;
use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
use cocoa::base::nil;
use cocoa::foundation::{NSInteger, NSString};
use rustyline::Editor;
use std::fs::File as SyncFile;
use std::io::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tide::prelude::*;
use tide::Body;

static USAGE: &str = include_str!("usage.txt");

unsafe fn get_change_count() -> NSInteger {
    let pb = NSPasteboard::generalPasteboard(nil);
    pb.changeCount()
}

unsafe fn get_current_entry() -> Option<String> {
    let pb = NSPasteboard::generalPasteboard(nil);
    let val = pb.stringForType(NSPasteboardTypeString);
    if val == nil {
        return None;
    }

    let bytes = val.UTF8String() as *const u8;
    Some(String::from(
        std::str::from_utf8(std::slice::from_raw_parts(bytes, val.len())).unwrap(),
    ))
}

unsafe fn set_current_entry(s: String) {
    let pb = NSPasteboard::generalPasteboard(nil);
    pb.clearContents();
    let val = NSString::alloc(nil).init_str(&s);
    pb.setString_forType(val, NSPasteboardTypeString);
}

async fn clipboard_sync(sender: Sender<clipr_common::Request>) {
    let mut last_hash: u64 = 0;
    let mut last_cc: i64 = 0;
    loop {
        task::sleep(Duration::from_millis(500)).await;
        let cc = unsafe { get_change_count() };
        if last_cc == cc {
            continue;
        } else {
            last_cc = cc;
        }
        match unsafe { get_current_entry() } {
            None => continue,
            Some(val) => {
                let hash = clipr_common::calculate_hash(&val);
                if last_hash == hash {
                    continue;
                }

                last_hash = hash;
                sender.send(clipr_common::Request::Sync(val)).await.unwrap();
            }
        }
    }
}

async fn repl_loop(sender: Sender<clipr_common::Request>) {
    let mut rl = Editor::<()>::new().unwrap();
    loop {
        let readline = rl.readline(":> ");
        match readline {
            Ok(line) => {
                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(line.as_str());

                let mut cmd_line = shellwords::split(line.as_str()).unwrap();
                let bin_name = std::env::args().next().unwrap();
                cmd_line.insert(0, bin_name);

                let cmd = match clipr_common::Args::try_parse_from(cmd_line) {
                    Ok(args) => args.command.unwrap(),
                    Err(_) => clipr_common::Command::Help,
                };

                match clipr_common::Request::send_cmd(&sender, cmd).await {
                    Some(clipr_common::Response::Stop) => return,
                    Some(clipr_common::Response::Payload(val)) => {
                        println!("{}", String::from(&val))
                    }
                    _ => continue,
                }
            }
            Err(_) => sender.send(clipr_common::Request::Quit).await.unwrap(),
        }
    }
}

async fn empty_fg_loop(sender: Sender<clipr_common::Request>) {
    let mut rl = Editor::<()>::new().unwrap();
    loop {
        let readline = rl.readline("");
        match readline {
            Ok(_) => continue,
            Err(_) => {
                sender.send(clipr_common::Request::Quit).await.unwrap();
            }
        }
    }
}

async fn http_server(listen_on: String, sender: Sender<clipr_common::Request>) -> Result<()> {
    let mut app = tide::with_state(sender);
    app.at("/command").post(
        |mut req: tide::Request<Sender<clipr_common::Request>>| async move {
            // TODO: handle invalid command properly
            let cmd: clipr_common::Command = req.body_json().await?;

            let sender = req.state();

            match clipr_common::Request::send_cmd(sender, cmd).await {
                Some(clipr_common::Response::Payload(val)) => Body::from_json(&val),
                _ => Body::from_json(&json!({})),
            }
        },
    );
    app.listen(listen_on).await?;
    Ok(())
}

async fn event_loop(state: Arc<clipr_common::State>, receiver: Receiver<clipr_common::Request>) {
    let s = state.clone();
    loop {
        if let Ok(msg) = receiver.recv().await {
            match msg {
                clipr_common::Request::Quit => return,
                clipr_common::Request::Sync(value) => {
                    let mut entries = s.entries.lock().unwrap();
                    entries.insert(value)
                }
                clipr_common::Request::Command(cmd, sender) => {
                    let payload = handle_call(s.clone(), cmd).await.unwrap();
                    match payload {
                        clipr_common::Payload::Stop => return,
                        _ => {
                            sender
                                .send(clipr_common::Response::Payload(payload))
                                .await
                                .unwrap();
                            continue;
                        }
                    }
                }
            };
        }
    }
}

async fn save_db(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = File::create(db_path).await?;
    let data = serde_json::to_string_pretty(&state.entries)?;
    file.write_all(data.as_bytes()).await?;
    Ok(())
}

fn save_db_sync(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = SyncFile::create(db_path)?;
    let data = serde_json::to_string_pretty(&state.entries)?;
    file.write_all(data.as_bytes())?;
    Ok(())
}

async fn load_db(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = File::open(db_path).await?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).await?;
    let data: clipr_common::Entries = serde_json::from_str(buffer.as_str())?;
    let mut entries = state.entries.lock().unwrap();
    *entries = data;
    drop(entries);
    Ok(())
}

async fn handle_call(
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
            save_db(state.clone()).await.unwrap();
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Load => {
            load_db(state.clone()).await.unwrap();
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
            unsafe { set_current_entry(value.join(" ")) };
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Insert { filename } => {
            let mut file = File::open(filename).await?;
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).await?;
            unsafe { set_current_entry(buffer) };
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Set { index } => {
            let mut entries = state.entries.lock().unwrap();
            if let Some(value) = entries.get_value(index) {
                unsafe { set_current_entry(value) };
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
            entries.pin(index, pin);
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Unpin { index } => {
            let mut entries = state.entries.lock().unwrap();
            entries.unpin(index);
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Select { value } => {
            let entries = state.entries.lock().unwrap();
            if value.len() < 2 {
                clipr_common::Payload::Message {
                    value: "invalid args".to_string(),
                }
            } else if value[0] == "value" {
                let items = entries.select_by_value((value[1]).to_string());
                clipr_common::Payload::List {
                    value: items,
                    preview_length: None,
                }
            } else if value[0] == "tag" {
                let items = entries.select_by_tag((value[1]).to_string());
                clipr_common::Payload::List {
                    value: items,
                    preview_length: None,
                }
            } else if value[0] == "pin" {
                let items = entries.select_by_pin((value[1]).to_string().chars().next().unwrap());
                clipr_common::Payload::List {
                    value: items,
                    preview_length: None,
                }
            } else {
                clipr_common::Payload::Ok
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

fn main() -> Result<()> {
    env_logger::init();
    let args = clipr_common::Args::parse();
    let config = clipr_common::Config::load_from_args(&args)?;
    let state = Arc::new(clipr_common::State::new(config));
    let (sender, receiver) = bounded::<clipr_common::Request>(1);
    task::spawn(clipboard_sync(sender.clone()));
    task::spawn(http_server(state.config.listen_on(), sender.clone()));
    if !state.config.interactive.unwrap_or(false) {
        task::spawn(empty_fg_loop(sender));
    } else {
        task::spawn(repl_loop(sender));
    }
    task::block_on(event_loop(state.clone(), receiver));
    // sync state at exit
    save_db_sync(state)?;
    Ok(())
}
