use async_std::channel::{bounded, Receiver, Sender};
use async_std::fs::File;
use async_std::io;
use async_std::net::TcpListener;
use async_std::prelude::*;
use async_std::task;
use chrono::prelude::*;
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

    let dt: DateTime<Local> = item.accessed_at.into();

    format!(
        "{:<64} #[{:<16}] @[{:<10}] ",
        val,
        tags,
        dt.format("%d-%m-%Y")
    )
}

fn _entries_to_indexed_vec(
    entries: &Entries,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Vec<(usize, &Item)> {
    let mut items: Vec<&Item> = entries.values().collect();

    items.sort_by_key(|i| i.accessed_at);
    items.reverse();

    let items_count = items.len();

    items
        .into_iter()
        .enumerate()
        .skip(offset.unwrap_or(0))
        .take(limit.unwrap_or(items_count))
        .collect()
}

fn dump_entries(entries: &Entries, limit: Option<usize>, offset: Option<usize>) -> String {
    let items = _entries_to_indexed_vec(entries, limit, offset);

    items
        .iter()
        .map(|(idx, item)| {
            format!(
                "{:?}: {}",
                idx + offset.unwrap_or(0),
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
    let items = _entries_to_indexed_vec(entries, None, None);

    items
        .iter()
        .find(|(i, _item)| idx == *i)
        .map(|(_, item)| item.value.clone())
}

fn del_entries(from_idx: usize, to_idx: Option<usize>, entries: &mut Entries) {
    let items = _entries_to_indexed_vec(entries, None, None);

    let hashes: Vec<u64> = items
        .iter()
        .filter(|(i, _item)| *i >= from_idx && *i <= to_idx.unwrap_or(from_idx))
        .map(|(_, item)| calculate_hash(&item.value))
        .collect();

    hashes.iter().for_each(|hash| {
        entries.remove(hash);
    });
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
    let items = _entries_to_indexed_vec(entries, None, None);

    items
        .into_iter()
        .filter(|(_, item)| item.value.contains(value.as_str()))
        .collect()
}

fn select_entries_by_tag(entries: &Entries, tag: String) -> Vec<(usize, &Item)> {
    let items = _entries_to_indexed_vec(entries, None, None);

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

fn _has_newlines(s: &str) -> Option<usize> {
    s.as_bytes()
        .iter()
        .enumerate()
        .find(|&(_, c)| *c == b'\n')
        .map(|(i, _)| i)
}

const MAX_LEN: usize = 64;
const SPACER_LEN: usize = 4;
const PREFIX_LEN: usize = 16;

fn shorten(s: &str) -> String {
    let chars = s.chars();
    let length = s.chars().count();

    // TODO:
    // 0. if has length > 64 -> S[0...PREFIX_LEN]...S[-PREFIX_LEN...]
    // 1. if has whitespaces until prefix-len -> S[0...PREFIX_LEN]...
    // 2. if has whitespaces after spacer -> S[0...PREFIX_LEN]...

    let mut short = if length > MAX_LEN {
        chars.enumerate().fold(String::new(), |acc, (i, c)| {
            if i < PREFIX_LEN || i > length - PREFIX_LEN {
                format!("{acc}{c}")
            } else if i > PREFIX_LEN && i < (PREFIX_LEN + SPACER_LEN) {
                format!("{acc}.")
            } else {
                acc
            }
        })
    } else {
        chars.collect::<String>()
    };

    let newline_offset = _has_newlines(short.as_str()).unwrap_or(short.len());
    short.replace_range(newline_offset.., "...");
    short
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

                let cmd = match Args::try_parse_from(cmd_line) {
                    Ok(args) => args.command.unwrap(),
                    Err(_) => Command::Help,
                };
                let (tx, rx) = bounded::<Response>(1);
                sender.send(Request::Command(cmd, tx)).await.unwrap();
                match rx.recv().await {
                    Ok(Response::Data(val)) => println!("{}", val),
                    Ok(Response::Stop) => return,
                    Ok(_) | Err(_) => continue,
                }
            }
            Err(_) => sender.send(Request::Quit).await.unwrap(),
        }
    }
}

async fn empty_fg_loop(_config: Arc<Config>, sender: Sender<Request>) {
    let mut rl = Editor::<()>::new().unwrap();
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
        let mut stream = stream?;
        let sender = sender.clone();
        task::spawn(async move {
            let cmd = read_command(&stream).await?;
            let (tx, rx) = bounded::<Response>(1);
            sender.send(Request::Command(cmd, tx)).await.unwrap();
            match rx.recv().await {
                Ok(Response::Data(val)) => {
                    stream.write_all(val.as_bytes()).await?;
                    stream.write(b"\n").await?;
                }
                Ok(_) | Err(_) => (),
            }
            Ok::<(), std::io::Error>(())
        });
    }

    Ok(())
}

async fn main_loop(config: Arc<Config>, receiver: Receiver<Request>) -> io::Result<()> {
    let mut entries = Entries::new();

    loop {
        if let Ok(msg) = receiver.recv().await {
            match msg {
                Request::Quit => Response::Stop,
                Request::Sync(value) => handle_insert(value, &mut entries),
                Request::Command(cmd, sender) => {
                    let response = handle_call(config.clone(), cmd, &mut entries)
                        .await
                        .unwrap();
                    match response {
                        Response::Stop => return Ok(()),
                        _ => {
                            sender.send(response).await.unwrap();
                            continue;
                        }
                    }
                }
            };
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
    Ok(match cmd {
        Command::List { limit, offset } => Response::Data(dump_entries(entries, limit, offset)),
        Command::Count => Response::Data(entries.len().to_string()),
        Command::Save => {
            save_db(config, entries).await.unwrap();
            Response::Ok
        }
        Command::Load => {
            load_db(config, entries).await.unwrap();
            Response::Ok
        }
        Command::Get { index } => {
            let result = match get_entry_value(index, entries) {
                Some(val) => val,
                None => format!("item at {:?} not found", index),
            };
            Response::Data(result)
        }
        Command::Add { value } => {
            unsafe { set_current_entry(value.join(" ")) };
            Response::Ok
        }
        Command::Insert { filename } => {
            let mut file = File::open(filename).await?;
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).await?;
            unsafe { set_current_entry(buffer) };
            Response::Ok
        }
        Command::Set { index } => {
            if let Some(value) = get_entry_value(index, entries) {
                unsafe { set_current_entry(value) };
                Response::Ok
            } else {
                Response::Data(format!("item at {:?} not found", index))
            }
        }
        Command::Del {
            from_index,
            to_index,
        } => {
            del_entries(from_index, to_index, entries);
            Response::Ok
        }
        Command::Tag { index, tag } => {
            if let Some(item) = get_entry(index, entries) {
                item.tags
                    .get_or_insert(HashSet::<String>::new())
                    .insert(tag);
                Response::Ok
            } else {
                Response::Data(format!("item at {:?} not found", index))
            }
        }
        Command::Select { value } => {
            if value.len() < 2 {
                Response::Data("invalid args".to_string())
            } else if value[0] == "value" {
                let items = select_entries_by_value(entries, (value[1]).to_string());
                Response::Data(dump_indexed_items(items))
            } else if value[0] == "tag" {
                let items = select_entries_by_tag(entries, (value[1]).to_string());
                Response::Data(dump_indexed_items(items))
            } else {
                Response::Ok
            }
        }

        Command::Help => {
            let usage: &str = "
  list ?limit ?offset
  count
  save
  load
  add -- str [?str...]
  del index ?to-index
  set index
  tag index tag
  get index
  insert filename
  select -- 'value'/'tag' str
  help
  quit";
            Response::Data(usage.to_string())
        }
        Command::Quit => Response::Stop,
    })
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
