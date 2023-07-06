use anyhow::Result;
use async_std::channel::{bounded, Receiver, Sender};
use async_std::fs::File;
use async_std::prelude::*;
use async_std::task;
use clap::Parser;
use clipr_common::Item;
use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use rustyline::Editor;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, LinkedList};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tide::prelude::*;
use tide::Body;

static USAGE: &str = include_str!("usage.txt");

unsafe fn nsstring_to_slice(s: &id) -> &str {
    let bytes = s.UTF8String() as *const u8;
    std::str::from_utf8(std::slice::from_raw_parts(bytes, s.len())).unwrap()
}

unsafe fn get_current_entry() -> String {
    let pb = NSPasteboard::generalPasteboard(nil);
    let val = pb.stringForType(NSPasteboardTypeString);
    nsstring_to_slice(&val).to_owned()
}

unsafe fn set_current_entry(s: String) {
    let pb = NSPasteboard::generalPasteboard(nil);
    pb.clearContents();
    let val = NSString::alloc(nil).init_str(&s);
    pb.setString_forType(val, NSPasteboardTypeString);
}

fn calculate_hash<T: Hash>(v: &T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn del_entries(from_idx: usize, to_idx: Option<usize>, entries: &mut clipr_common::Entries) {
    let removed: LinkedList<Item> = match to_idx {
        None => entries.values.split_off(from_idx),
        Some(to_idx) => {
            let mut upper = entries.values.split_off(from_idx);
            let removed = upper.split_off(to_idx - from_idx);
            entries.values.append(&mut upper);
            removed
        }
    };

    removed.iter().for_each(|item| {
        entries.hashes.remove(&calculate_hash(&item.value));
    });
}

fn get_entry(idx: usize, entries: &mut clipr_common::Entries) -> Option<&mut clipr_common::Item> {
    entries
        .values
        .iter_mut()
        .enumerate()
        .find(|(i, _)| idx == *i)
        .map(|(_, item)| item)
}

fn get_entry_value(idx: usize, entries: &mut clipr_common::Entries) -> Option<String> {
    get_entry(idx, entries).map(|item| item.value.clone())
}

fn select_entries(
    entries: &clipr_common::Entries,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Vec<(usize, clipr_common::Item)> {
    let offset_val = offset.unwrap_or(0);
    let limit_val = limit.unwrap_or(entries.values.len());

    entries
        .values
        .iter()
        .enumerate()
        .filter(|(idx, item)| *idx >= offset_val && *idx <= (offset_val + limit_val))
        .map(|(idx, item)| ((idx + offset_val), item.clone()))
        .collect()
}

fn select_entries_by_value(
    entries: &clipr_common::Entries,
    value: String,
) -> Vec<(usize, clipr_common::Item)> {
    let val = value.as_str();

    entries
        .values
        .iter()
        .enumerate()
        .filter(|(_, item)| item.value.contains(val))
        .map(|(idx, item)| (idx, item.clone()))
        .collect()
}

fn select_entries_by_tag(
    entries: &clipr_common::Entries,
    tag: String,
) -> Vec<(usize, clipr_common::Item)> {
    entries
        .values
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            if let Some(tags) = &item.tags {
                tags.get(&tag).is_some()
            } else {
                false
            }
        })
        .map(|(idx, item)| (idx, item.clone()))
        .collect()
}

fn get_entries_tags(entries: &clipr_common::Entries) -> HashSet<String> {
    let mut result: HashSet<String> = HashSet::new();
    for item in entries.values.iter() {
        if let Some(tags) = item.tags.as_ref() {
            result = result.union(tags).cloned().collect();
        }
    }
    result
}

async fn clipboard_sync(sender: Sender<clipr_common::Request>) {
    let mut last_hash: u64 = 0;
    loop {
        task::sleep(Duration::from_millis(500)).await;
        let val = unsafe { get_current_entry() };
        if val.is_empty() {
            continue;
        }

        let hash = calculate_hash(&val);
        if last_hash == hash {
            continue;
        }

        last_hash = hash;
        sender.send(clipr_common::Request::Sync(val)).await.unwrap();
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
                    handle_insert(value, &mut entries)
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

fn handle_insert(s: String, entries: &mut clipr_common::Entries) {
    let hash = calculate_hash(&s);

    if entries.hashes.contains(&hash) {
        if let Some(idx) = entries
            .values
            .iter()
            .enumerate()
            .find(|(_, item)| calculate_hash(&item.value) == hash)
            .map(|(idx, _)| idx)
        {
            let mut tail = entries.values.split_off(idx);
            if let Some(mut elt) = tail.pop_front() {
                elt.access_counter += 1;

                entries.values.push_front(elt);
                entries.values.append(&mut tail);
            }
        }
    } else {
        entries.hashes.insert(hash);
        entries.values.push_front(clipr_common::Item {
            value: s,
            access_counter: 1,
            tags: None,
        })
    }
}

async fn save_db(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = File::create(db_path).await?;
    let data = serde_lexpr::to_string_custom(&state.entries, serde_lexpr::print::Options::elisp())?;
    file.write_all(data.as_bytes()).await?;
    Ok(())
}

async fn load_db(state: Arc<clipr_common::State>) -> Result<()> {
    let db_path = state.config.db.as_ref().unwrap();
    let mut file = File::open(db_path).await?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).await?;
    let data: clipr_common::Entries =
        serde_lexpr::from_str_custom(buffer.as_str(), serde_lexpr::parse::Options::elisp())?;
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
            limit,
            offset,
            preview_length,
        } => {
            let entries = state.entries.lock().unwrap();
            let items = select_entries(&entries, limit, offset);
            clipr_common::Payload::List {
                value: items,
                preview_length,
            }
        }
        clipr_common::Command::Count => {
            let entries = state.entries.lock().unwrap();
            clipr_common::Payload::Value {
                value: Some(entries.values.len().to_string()),
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
            match get_entry_value(index, &mut entries) {
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
            if let Some(value) = get_entry_value(index, &mut entries) {
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
            del_entries(from_index, to_index, &mut entries);
            clipr_common::Payload::Ok
        }
        clipr_common::Command::Tag { index, tag } => {
            let mut entries = state.entries.lock().unwrap();
            if let Some(item) = get_entry(index, &mut entries) {
                item.tags
                    .get_or_insert(HashSet::<String>::new())
                    .insert(tag);
                clipr_common::Payload::Ok
            } else {
                clipr_common::Payload::Message {
                    value: format!("item at {index:?} not found"),
                }
            }
        }
        clipr_common::Command::Untag { index, tag } => {
            let mut entries = state.entries.lock().unwrap();
            if let Some(item) = get_entry(index, &mut entries) {
                match item.tags.as_mut() {
                    Some(ts) => ts.remove(&tag),
                    _ => true,
                };
                clipr_common::Payload::Ok
            } else {
                clipr_common::Payload::Message {
                    value: format!("item at {index:?} not found"),
                }
            }
        }
        clipr_common::Command::Select { value } => {
            let entries = state.entries.lock().unwrap();
            if value.len() < 2 {
                clipr_common::Payload::Message {
                    value: "invalid args".to_string(),
                }
            } else if value[0] == "value" {
                let items = select_entries_by_value(&entries, (value[1]).to_string());
                clipr_common::Payload::List {
                    value: items,
                    preview_length: None,
                }
            } else if value[0] == "tag" {
                let items = select_entries_by_tag(&entries, (value[1]).to_string());
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
            let tags = get_entries_tags(&entries);
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
    task::block_on(event_loop(state, receiver));
    Ok(())
}
