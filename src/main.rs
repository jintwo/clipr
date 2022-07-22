use async_std::channel::{bounded, Receiver, Sender};
use async_std::io;
use async_std::net::TcpListener;
use async_std::prelude::*;
use async_std::task;
use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use rustyline::Editor;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::prelude::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

mod common;
use common::{load_config, read_command, Command, CommandParseError, Config, Request, Response};

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

struct Item {
    value: String,
    accessed_at: Instant,
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

    format!("{:?} tags: {{{}}}", val, tags)
}

fn _entries_to_vec(entries: &Entries) -> Vec<&Item> {
    let mut items: Vec<&Item> = entries.values().collect();

    items.sort_by_key(|i| i.accessed_at);
    items.reverse();
    items
}

fn dump_entries(entries: &Entries) -> String {
    let items = _entries_to_vec(entries);

    items
        .iter()
        .enumerate()
        .map(|(idx, item)| format!("{:?}: {}", idx, _format_item(item, true)))
        .collect::<Vec<String>>()
        .join("\n")
}

fn get_entry_value(idx: u32, entries: &Entries) -> Option<String> {
    let items = _entries_to_vec(entries);

    items
        .iter()
        .enumerate()
        .find(|(i, _item)| idx == (*i).try_into().unwrap())
        .map(|(_, item)| item.value.clone())
}

fn del_entry(idx: u32, entries: &mut Entries) -> Option<Item> {
    if let Some(value) = get_entry_value(idx, entries) {
        let hash = calculate_hash(value);
        entries.remove(&hash)
    } else {
        None
    }
}

fn get_entry(idx: u32, entries: &mut Entries) -> Option<&mut Item> {
    if let Some(value) = get_entry_value(idx, entries) {
        let hash = calculate_hash(value);
        entries.get_mut(&hash)
    } else {
        None
    }
}

fn shorten(s: &String) -> String {
    let mut res = s.clone();

    if res.len() > 64 {
        res.replace_range(16..(s.len() - 16), "...");
    }

    res
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
        let cmd = match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                match line.parse::<Command>() {
                    Err(CommandParseError::EmptyCommand) => continue,
                    Ok(cmd) => cmd,
                    Err(err) => Command::Invalid(err),
                }
            }
            Err(_) => Command::Quit,
        };
        sender
            .send(Request::CmdLine(cmd, io::stdout()))
            .await
            .unwrap();
    }
}

async fn empty_fg_loop(_config: Arc<Config>, sender: Sender<Request>) {
    let mut rl = Editor::<()>::new();
    loop {
        let readline = rl.readline("");
        let cmd = match readline {
            Ok(_) => continue,
            Err(_) => Command::Quit,
        };
        sender
            .send(Request::CmdLine(cmd, io::stdout()))
            .await
            .unwrap();
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

async fn main_loop(_config: Arc<Config>, receiver: Receiver<Request>) -> io::Result<()> {
    let mut entries = Entries::new();

    loop {
        if let Ok(msg) = receiver.recv().await {
            let response = match msg {
                Request::Sync(value) => handle_insert(value, &mut entries),
                Request::CmdLine(cmd, mut stream) => {
                    let rep = handle_call(cmd, &mut entries);
                    write_response(&mut stream, &rep).await?;
                    rep
                }
                Request::Net(cmd, mut stream) => {
                    let rep = handle_call(cmd, &mut entries);
                    write_response(&mut stream, &rep).await?;
                    rep
                }
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
            item.accessed_at = Instant::now();
            item.access_counter += 1;
            Response::Ok
        }
        None => {
            let now = Instant::now();
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

fn show_help() -> String {
    "available commands...".to_string()
}

fn handle_call(cmd: Command, entries: &mut Entries) -> Response {
    match cmd {
        Command::Quit => Response::Stop,
        Command::Help => Response::Data(show_help()),
        Command::List => Response::Data(dump_entries(entries)),
        Command::Get(idx) => {
            let result = match get_entry_value(idx, entries) {
                Some(val) => val,
                None => format!("item at {:?} not found", idx),
            };
            Response::Data(result)
        }
        Command::Add(value) => {
            unsafe { set_current_entry(value) };
            Response::Ok
        }
        Command::Load(filename) => {
            let mut file = File::open(filename).unwrap();
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).unwrap();
            unsafe { set_current_entry(buffer) };
            Response::Ok
        }
        Command::Set(idx) => {
            if let Some(value) = get_entry_value(idx, entries) {
                unsafe { set_current_entry(value) };
                Response::Ok
            } else {
                Response::Data(format!("item at {:?} not found", idx))
            }
        }
        Command::Del(idx) => {
            if del_entry(idx, entries).is_none() {
                Response::Data(format!("item at {:?} not found", idx))
            } else {
                Response::Ok
            }
        }
        Command::Tag(idx, tag) => {
            if let Some(mut item) = get_entry(idx, entries) {
                if item.tags.is_none() {
                    let mut tags = HashSet::<String>::new();
                    tags.insert(tag);
                    item.tags = Some(tags);
                } else {
                    let tags = item.tags.as_mut().unwrap();
                    tags.insert(tag);
                }
                Response::Ok
            } else {
                Response::Data(format!("item at {:?} not found", idx))
            }
        }
        Command::Invalid(e) => Response::Data(format!("error: {:?}", e)),
    }
}

fn main() -> io::Result<()> {
    let args = env::args();
    let config = Arc::new(if args.len() < 2 {
        Config::default()
    } else {
        let config_filename = args.skip(1).nth(0).unwrap();
        let config = load_config(config_filename.as_str())?;
        config
    });

    let (sender, receiver) = bounded::<Request>(1);
    task::spawn(sync_loop(config.clone(), sender.clone()));
    task::spawn(net_loop(config.clone(), sender.clone()));
    task::spawn(cmdline_loop(config.clone(), sender));
    task::block_on(main_loop(config.clone(), receiver))?;
    Ok(())
}
