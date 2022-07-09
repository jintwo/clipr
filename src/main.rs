use async_std::channel::{bounded, Receiver, Sender};
use async_std::task;
use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
use cocoa::base::{id, nil};
use cocoa::foundation::NSString;
use rustyline::Editor;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::prelude::*;
use std::str::FromStr;
use std::time::{Duration, Instant};

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
    created_at: Instant,
    accessed_at: Instant,
    access_counter: u32,
    tags: Option<HashSet<String>>,
}

#[derive(Debug)]
enum CommandParseError {
    EmptyCommand,
    InvalidCommand(String),
    InsufficientArgs,
    InvalidArgType(String),
}

#[derive(Debug)]
enum Command {
    Add(String),
    List,
    Show(u32),
    Set(u32),
    Load(String),
    Tag(u32, String),
    Quit,
    Invalid(CommandParseError),
}

enum Message {
    Insert(String),
    Call(Command),
}

impl FromStr for Command {
    type Err = CommandParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts: VecDeque<&str> = s.split(' ').collect();

        let cmd = parts.pop_front().unwrap();
        if cmd.is_empty() {
            return Err(CommandParseError::EmptyCommand);
        }

        match cmd {
            "list" => Ok(Command::List),
            "quit" => Ok(Command::Quit),
            "add" | "show" | "set" | "load" if parts.is_empty() => {
                Err(CommandParseError::InsufficientArgs)
            }
            "add" => {
                let args = parts.make_contiguous().join(" ");
                Ok(Command::Add(args))
            }
            "show" => {
                if let Ok(arg) = parts[0].parse() {
                    Ok(Command::Show(arg))
                } else {
                    Err(CommandParseError::InvalidArgType(parts[0].to_owned()))
                }
            }
            "set" => {
                if let Ok(arg) = parts[0].parse() {
                    Ok(Command::Set(arg))
                } else {
                    Err(CommandParseError::InvalidArgType(parts[0].to_owned()))
                }
            }
            "load" => {
                let filename = parts[0].to_owned();
                Ok(Command::Load(filename))
            }
            "tag" if parts.len() < 2 => Err(CommandParseError::InsufficientArgs),
            "tag" => {
                if let Ok(idx) = parts[0].parse() {
                    let tag = parts[1].to_owned();
                    Ok(Command::Tag(idx, tag))
                } else {
                    Err(CommandParseError::InvalidArgType(parts[0].to_owned()))
                }
            }
            _ => Err(CommandParseError::InvalidCommand(cmd.to_owned())),
        }
    }
}

fn dump_entries(entries: &Entries) {
    let mut items: Vec<&Item> = entries.values().collect();

    items.sort_by_key(|i| i.accessed_at);
    items.reverse();

    items.iter().enumerate().for_each(|(idx, item)| {
        println!("{:?}: {:?} tags:{:?}", idx, shorten(&item.value), item.tags)
    });
}

fn get_entry_value(idx: u32, entries: &Entries) -> Option<String> {
    let mut items: Vec<&Item> = entries.values().collect();

    items.sort_by_key(|i| i.accessed_at);
    items.reverse();

    items
        .iter()
        .enumerate()
        .find(|(i, _item)| idx == (*i).try_into().unwrap())
        .map(|(_, item)| item.value.clone())
}

fn get_entry(idx: u32, entries: &mut Entries) -> Option<&mut Item> {
    if let Some(value) = get_entry_value(idx, entries) {
        let hash = calculate_hash(value);
        entries.get_mut(&hash)
    } else {
        None
    }
}

fn show_entry(idx: u32, entries: &Entries) {
    let mut items: Vec<&Item> = entries.values().collect();

    items.sort_by_key(|i| i.accessed_at);
    items.reverse();

    match items
        .iter()
        .enumerate()
        .find(|(i, _item)| idx == (*i).try_into().unwrap())
    {
        Some((_, item)) => println!("{:?}: {:?} tags: {:?}", idx, &item.value, item.tags),
        None => println!("item at {:?} not found", idx),
    }
}

fn shorten(s: &String) -> String {
    if s.len() > 64 {
        let mut res = s.clone();
        res.replace_range(16..(s.len() - 16), "...");
        res
    } else {
        s.clone()
    }
}

async fn sync_loop(sender: Sender<Message>) {
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
        sender.send(Message::Insert(val)).await.unwrap();
    }
}

async fn cmd_loop(sender: Sender<Message>) {
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
        sender.send(Message::Call(cmd)).await.unwrap();
    }
}

async fn net_loop(_sender: Sender<Message>) {
    loop {
        task::sleep(Duration::from_secs(1)).await;
        println!("net-loop is sleeping...")
    }
}

async fn main_loop(receiver: Receiver<Message>) {
    let mut entries = Entries::new();

    loop {
        if let Ok(msg) = receiver.recv().await {
            match msg {
                Message::Insert(value) => handle_insert(value, &mut entries),
                Message::Call(cmd) => {
                    if !handle_call(cmd, &mut entries) {
                        return;
                    }
                }
            };
        }
    }
}

fn handle_insert(s: String, entries: &mut Entries) {
    let hash = calculate_hash(&s);

    match entries.get_mut(&hash) {
        Some(item) => {
            item.accessed_at = Instant::now();
            item.access_counter += 1;
        }
        None => {
            let now = Instant::now();
            entries.insert(
                hash,
                Item {
                    value: s,
                    created_at: now,
                    accessed_at: now,
                    access_counter: 1,
                    tags: None,
                },
            );
        }
    };
}

fn handle_call(cmd: Command, entries: &mut Entries) -> bool {
    match cmd {
        Command::Quit => false,
        Command::List => {
            dump_entries(entries);
            true
        }
        Command::Show(idx) => {
            show_entry(idx, entries);
            true
        }
        Command::Add(value) => {
            unsafe { set_current_entry(value) };
            true
        }
        Command::Load(filename) => {
            let mut file = File::open(filename).unwrap();
            let mut buffer = String::new();
            file.read_to_string(&mut buffer).unwrap();
            unsafe { set_current_entry(buffer) };
            true
        }
        Command::Set(idx) => {
            if let Some(value) = get_entry_value(idx, entries) {
                unsafe { set_current_entry(value) }
            } else {
                println!("item at {:?} not found", idx)
            }
            true
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
            } else {
                println!("item at {:?} not found", idx)
            }
            true
        }
        Command::Invalid(e) => {
            println!("error: {:?}", e);
            true
        }
    }
}

fn main() {
    let (sender, receiver) = bounded::<Message>(1);
    task::spawn(sync_loop(sender.clone()));
    task::spawn(net_loop(sender.clone()));
    task::spawn(cmd_loop(sender));
    task::block_on(main_loop(receiver));
}
