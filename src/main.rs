use async_std::channel::{bounded, Receiver, Sender};
use async_std::fs::File;
use async_std::io;
use async_std::net::TcpListener;
use async_std::prelude::*;
use async_std::task;
use clap::Parser;
use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use rustyline::Editor;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

mod common;
use common::{read_command, Args, Command, Config, Request, Response};

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

fn calculate_hash<T: Hash>(v: T) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

type Entries = std::collections::BTreeMap<u64, Item>;

#[derive(serde_derive::Serialize, serde_derive::Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Item {
    value: String,
    accessed_at: SystemTime,
    access_counter: u32,
    tags: Option<HashSet<String>>,
}

fn _format_item(item: &Item, short: bool) -> String {
    let val = if short {
        shorten(&item.value)
    } else {
        item.value.clone()
    };

    let tags = match &item.tags {
        Some(tags) => tags
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<&str>>()
            .join(","),
        None => "".to_string(),
    };

    format!("{:?} tags: [{}]", val, tags)
}

fn _entries_to_indexed_vec(entries: &Entries, offset: Option<usize>) -> Vec<(usize, &Item)> {
    let mut items: Vec<&Item> = entries.values().collect();

    items.sort_by_key(|i| i.accessed_at);
    items.reverse();

    let items_indexed: Vec<(usize, &Item)> = items.into_iter().enumerate().collect();

    if let Some(offset) = offset {
        items_indexed.into_iter().skip(offset as usize).collect()
    } else {
        items_indexed
    }
}

fn dump_entries(entries: &Entries, offset: Option<usize>) -> String {
    let items = _entries_to_indexed_vec(entries, offset);

    items
        .iter()
        .map(|(idx, item)| {
            format!(
                "{:?}: {}",
                idx + offset.or(Some(0)).unwrap(),
                _format_item(item, true)
            )
        })
        .collect::<Vec<String>>()
        .join("\n")
}

fn dump_indexed_items(items: Vec<(usize, &Item)>) -> String {
    items
        .iter()
        .map(|(idx, item)| format!("{:?}: {}", idx, _format_item(item, true)))
        .collect::<Vec<String>>()
        .join("\n")
}

fn get_entry_value(idx: usize, entries: &Entries) -> Option<String> {
    let items = _entries_to_indexed_vec(entries, None);

    items
        .iter()
        .find(|(i, _item)| idx == *i)
        .map(|(_, item)| item.value.clone())
}

fn del_entry(idx: usize, entries: &mut Entries) -> Option<Item> {
    if let Some(value) = get_entry_value(idx, entries) {
        let hash = calculate_hash(value);
        entries.remove(&hash)
    } else {
        None
    }
}

fn get_entry(idx: usize, entries: &mut Entries) -> Option<&mut Item> {
    if let Some(value) = get_entry_value(idx, entries) {
        let hash = calculate_hash(value);
        entries.get_mut(&hash)
    } else {
        None
    }
}

fn select_entries_by_value(entries: &Entries, value: String) -> Vec<(usize, &Item)> {
    let items = _entries_to_indexed_vec(entries, None);

    items
        .into_iter()
        .filter(|(_, item)| item.value.contains(value.as_str()))
        .collect()
}

fn select_entries_by_tag(entries: &Entries, tag: String) -> Vec<(usize, &Item)> {
    let items = _entries_to_indexed_vec(entries, None);

    items
        .into_iter()
        .filter(|(_, item)| {
            if let Some(tags) = &item.tags {
                tags.get(&tag).is_some()
            } else {
                false
            }
        })
        .collect()
}

fn shorten(s: &String) -> String {
    let chars = s.chars();
    let length = s.chars().count();

    if length > 64 {
        chars.enumerate().fold(String::new(), |acc, (i, c)| {
            if i < 16 || i > length - 16 {
                format!("{acc}{c}")
            } else if i > 16 && i < 20 {
                format!("{acc}.")
            } else {
                acc
            }
        })
    } else {
        chars.collect::<String>()
    }
}

async fn sync_loop(_config: Arc<Config>, sender: Sender<Request>) {
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
        sender.send(Request::Sync(val)).await.unwrap();
    }
}

async fn repl_loop(_config: Arc<Config>, sender: Sender<Request>) {
    let mut rl = Editor::<()>::new();
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
                if let Ok(args) = Args::try_parse_from(cmd_line) {
                    let cmd = args.command.unwrap();
                    sender
                        .send(Request::CmdLine(cmd, io::stdout()))
                        .await
                        .unwrap();
                } else {
                    println!("invalid command");
                    continue;
                }
            }
            Err(_) => sender.send(Request::Quit).await.unwrap(),
        }
    }
}

async fn empty_fg_loop(_config: Arc<Config>, sender: Sender<Request>) {
    let mut rl = Editor::<()>::new();
    loop {
        let readline = rl.readline("");
        match readline {
            Ok(_) => continue,
            Err(_) => {
                sender.send(Request::Quit).await.unwrap();
            }
        }
    }
}

async fn cmdline_loop(config: Arc<Config>, sender: Sender<Request>) {
    if !config.interactive.unwrap_or(false) {
        empty_fg_loop(config, sender).await;
    } else {
        repl_loop(config, sender).await;
    };
}

async fn net_loop(config: Arc<Config>, sender: Sender<Request>) -> io::Result<()> {
    let listen_on = format!(
        "{}:{}",
        &config.host.as_ref().unwrap(),
        &config.port.unwrap()
    );
    let listener = TcpListener::bind(listen_on).await?;

    let mut incoming = listener.incoming();

    while let Some(stream) = incoming.next().await {
        let stream = stream?;
        let sender = sender.clone();
        task::spawn(async move {
            let command = read_command(&stream).await?;
            let request = Request::Net(command, stream);
            sender.send(request).await.unwrap();
            Ok::<(), std::io::Error>(())
        });
    }

    Ok(())
}

async fn write_response<W: io::WriteExt + std::marker::Unpin>(
    stream: &mut W,
    response: &Response,
) -> io::Result<()> {
    if let Response::Data(val) = response {
        stream.write_all(val.as_bytes()).await?;
        stream.write(b"\n").await?;
    }
    Ok(())
}

async fn main_loop(config: Arc<Config>, receiver: Receiver<Request>) -> io::Result<()> {
    let mut entries = Entries::new();

    loop {
        if let Ok(msg) = receiver.recv().await {
            let response = match msg {
                Request::Sync(value) => handle_insert(value, &mut entries),
                Request::CmdLine(cmd, mut stream) => {
                    let rep = handle_call(config.clone(), cmd, &mut entries).await?;
                    write_response(&mut stream, &rep).await?;
                    rep
                }
                Request::Net(cmd, mut stream) => {
                    let rep = handle_call(config.clone(), cmd, &mut entries).await?;
                    write_response(&mut stream, &rep).await?;
                    rep
                }
                Request::Quit => Response::Stop,
            };

            match response {
                Response::Stop => return Ok(()),
                Response::Ok | Response::NewItem(_) | Response::Data(_) => continue,
            }
        }
    }
}

fn handle_insert(s: String, entries: &mut Entries) -> Response {
    let hash = calculate_hash(&s);

    match entries.get_mut(&hash) {
        Some(item) => {
            item.accessed_at = SystemTime::now();
            item.access_counter += 1;
            Response::Ok
        }
        None => {
            let now = SystemTime::now();
            entries.insert(
                hash,
                Item {
                    value: s.clone(),
                    accessed_at: now,
                    access_counter: 1,
                    tags: None,
                },
            );
            Response::NewItem(s)
        }
    }
}
async fn save_db(config: Arc<Config>, entries: &Entries) -> io::Result<()> {
    let db_path = config.db.as_ref().unwrap();
    let mut file = File::create(db_path).await?;
    let data = serde_lexpr::to_string_custom(entries, serde_lexpr::print::Options::elisp())?;
    file.write_all(data.as_bytes()).await?;
    Ok(())
}

async fn load_db(config: Arc<Config>, entries: &mut Entries) -> io::Result<()> {
    let db_path = config.db.as_ref().unwrap();
    let mut file = File::open(db_path).await?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).await?;
    let mut data: Entries =
        serde_lexpr::from_str_custom(buffer.as_str(), serde_lexpr::parse::Options::elisp())?;
    entries.append(&mut data);
    Ok(())
}

async fn handle_call(
    config: Arc<Config>,
    cmd: Command,
    entries: &mut Entries,
) -> io::Result<Response> {
    match cmd {
        Command::List { offset } => Ok(Response::Data(dump_entries(entries, offset))),
        Command::Count => Ok(Response::Data(entries.len().to_string())),
        Command::Save => {
            save_db(config, entries).await.unwrap();
            Ok(Response::Ok)
        }
        Command::Load => {
            load_db(config, entries).await.unwrap();
            Ok(Response::Ok)
        }
        Command::Get { index } => {
            let result = match get_entry_value(index, entries) {
                Some(val) => val,
                None => format!("item at {:?} not found", index),
            };
            Ok(Response::Data(result))
        }
        Command::Add { value } => {
            unsafe { set_current_entry(value.join(" ")) };
            Ok(Response::Ok)
        }
        Command::Insert { filename } => {
            let mut file = File::open(filename).await?;
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).await?;
            unsafe { set_current_entry(buffer) };
            Ok(Response::Ok)
        }
        Command::Set { index } => {
            if let Some(value) = get_entry_value(index, entries) {
                unsafe { set_current_entry(value) };
                Ok(Response::Ok)
            } else {
                Ok(Response::Data(format!("item at {:?} not found", index)))
            }
        }
        Command::Del { index } => {
            if del_entry(index, entries).is_none() {
                Ok(Response::Data(format!("item at {:?} not found", index)))
            } else {
                Ok(Response::Ok)
            }
        }
        Command::Tag { index, tag } => {
            if let Some(item) = get_entry(index, entries) {
                item.tags
                    .get_or_insert(HashSet::<String>::new())
                    .insert(tag);
                Ok(Response::Ok)
            } else {
                Ok(Response::Data(format!("item at {:?} not found", index)))
            }
        }
        Command::Select { value } => {
            if value.len() < 2 {
                return Ok(Response::Data("invalid args".to_string()));
            }
            Ok(if value[0] == "value" {
                let items = select_entries_by_value(entries, (&value[1]).to_string());
                Response::Data(dump_indexed_items(items))
            } else if value[0] == "tag" {
                let items = select_entries_by_tag(entries, (&value[1]).to_string());
                Response::Data(dump_indexed_items(items))
            } else {
                Response::Ok
            })
        }
    }
}

fn main() -> io::Result<()> {
    let args = Args::parse();
    let config = Config::load_from_args(&args)?;

    let (sender, receiver) = bounded::<Request>(1);
    task::spawn(sync_loop(config.clone(), sender.clone()));
    task::spawn(net_loop(config.clone(), sender.clone()));
    task::spawn(cmdline_loop(config.clone(), sender));
    task::block_on(main_loop(config, receiver))?;
    Ok(())
}
