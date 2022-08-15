use async_std::io;
use async_std::net::TcpStream;
use async_std::prelude::*;
use clap::{Parser, Subcommand};
use serde_derive::Deserialize;
use std::fs::File;
use std::io::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

pub const HEADER_LEN: usize = 8;

#[derive(Debug)]
pub enum Request {
    Sync(String),
    Quit,
    CmdLine(Command, io::Stdout),
    Net(Command, TcpStream),
}

#[derive(Debug)]
pub enum Response {
    Data(String),
    NewItem(String),
    Ok,
    Stop,
}

#[derive(Error, Debug)]
pub enum CommandParseError {
    #[error("empty command")]
    EmptyCommand,
    #[error("invalid command `{0}`")]
    InvalidCommand(String),
    #[error("insufficient arguments")]
    InsufficientArgs,
    #[error("invalid argument type `{0}`")]
    InvalidArgType(String),
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Add {
        #[clap(last = true)]
        value: Vec<String>,
    },
    Del {
        index: u32,
    },
    List {
        offset: Option<u32>,
    },
    Get {
        index: u32,
    },
    Set {
        index: u32,
    },
    Insert {
        filename: String,
    },
    Tag {
        index: u32,
        tag: String,
    },
    Count,
    Save,
    Load,
    Select {
        #[clap(last = true)]
        value: Vec<String>,
    },
}

impl From<CommandParseError> for std::io::Error {
    fn from(cpe: CommandParseError) -> Self {
        io::Error::new(io::ErrorKind::Other, format!("{:?}", cpe))
    }
}

fn command_to_vec(cmd: &Command) -> Vec<u8> {
    let s = match cmd {
        Command::List { offset } => match offset {
            Some(offset) => format!("list {}", offset),
            None => "list".to_string(),
        },
        Command::Count => "count".to_string(),
        Command::Save => "save".to_string(),
        Command::Load => "load".to_string(),
        Command::Add { value } => format!("add -- {}", value.join(" ")),
        Command::Del { index } => format!("del {}", index),
        Command::Set { index } => format!("set {}", index),
        Command::Tag { index, tag } => format!("tag {} {}", index, tag),
        Command::Get { index } => format!("get {}", index),
        Command::Insert { filename } => format!("insert {}", filename),
        Command::Select { value } => format!("select -- {}", value.join(" ")),
    };

    s.as_bytes().to_vec()
}

impl From<Command> for Vec<u8> {
    fn from(cmd: Command) -> Self {
        command_to_vec(&cmd)
    }
}

impl From<&Command> for Vec<u8> {
    fn from(cmd: &Command) -> Self {
        command_to_vec(cmd)
    }
}

pub async fn read_command(stream: &TcpStream) -> io::Result<Command> {
    let mut reader = stream.clone();

    // read header
    let mut header: [u8; HEADER_LEN] = [0; HEADER_LEN];
    reader.read_exact(&mut header).await?;
    let buf_len = usize::from_le_bytes(header);

    // read payload
    let mut buf = vec![0u8; buf_len];
    reader.read_exact(&mut buf).await?;
    let payload = String::from_utf8_lossy(&buf[..]);

    // parse command
    let mut cmd_line = shellwords::split(&payload).unwrap();
    let bin_name = std::env::args().next().unwrap();
    cmd_line.insert(0, bin_name);

    if let Ok(args) = Args::try_parse_from(cmd_line) {
        let cmd = args.command.unwrap();
        Ok(cmd)
    } else {
        Err(CommandParseError::InvalidCommand(payload.to_string()).into())
    }
}

pub async fn write_command(stream: &mut TcpStream, cmd: Command) -> io::Result<()> {
    // encode payload
    let payload: Vec<u8> = cmd.into();

    // write header
    let buf_len = payload.len();
    let header = &buf_len.to_le_bytes()[..HEADER_LEN];
    stream.write_all(header).await?;

    // write payload
    stream.write_all(&payload[..]).await?;
    stream.flush().await?;

    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub interactive: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub db: Option<String>,
}

#[derive(Parser, Debug)]
pub struct Args {
    #[clap(short, long, value_parser)]
    pub config: Option<PathBuf>,
    #[clap(subcommand)]
    pub command: Option<Command>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: Some(String::from("127.0.0.1")),
            port: Some(8931),
            interactive: Some(true),
            db: Some(String::from("./db.lisp")),
        }
    }
}

impl Config {
    pub fn load_config(filename: &Path) -> io::Result<Config> {
        let mut file = File::open(filename)?;
        let mut buffer = String::new();
        file.read_to_string(&mut buffer)?;

        let config: Config = toml::from_str(buffer.as_str())?;

        Ok(config)
    }

    pub fn load_from_args(args: &Args) -> io::Result<Arc<Self>> {
        Ok(Arc::new(if let Some(filename) = args.config.as_deref() {
            Self::load_config(filename)?
        } else {
            Self::default()
        }))
    }
}
