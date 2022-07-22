use async_std::io;
use async_std::net::TcpStream;
use async_std::prelude::*;
use serde_derive::Deserialize;
use std::collections::VecDeque;
use std::fs::File;
use std::io::prelude::*;
use std::str::FromStr;
use thiserror::Error;

pub const HEADER_LEN: usize = 8;

#[derive(Debug)]
pub enum Request {
    Sync(String),
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

#[derive(Debug)]
pub enum Command {
    Add(String),
    Del(u32),
    List,
    Show(u32),
    Set(u32),
    Load(String),
    Tag(u32, String),
    Quit,
    Invalid(CommandParseError),
    Help,
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
            "help" => Ok(Command::Help),
            "quit" => Ok(Command::Quit),
            "add" | "show" | "set" | "load" | "del" if parts.is_empty() => {
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
            "del" => {
                if let Ok(arg) = parts[0].parse() {
                    Ok(Command::Del(arg))
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

impl From<Command> for Vec<u8> {
    fn from(cmd: Command) -> Self {
        let s = match cmd {
            Command::List | Command::Invalid(_) => "list".to_string(),
            Command::Help => "help".to_string(),
            Command::Quit => "quit".to_string(),
            Command::Add(v) => format!("add {}", v),
            Command::Del(i) => format!("del {}", i),
            Command::Set(i) => format!("set {}", i),
            Command::Tag(i, v) => format!("tag {} {}", i, v),
            Command::Show(i) => format!("show {}", i),
            Command::Load(v) => format!("load {}", v),
        };

        s.as_bytes().to_vec()
    }
}

impl From<&Command> for Vec<u8> {
    fn from(cmd: &Command) -> Self {
        let s = match cmd {
            Command::List | Command::Invalid(_) => "list".to_string(),
            Command::Help => "help".to_string(),
            Command::Quit => "quit".to_string(),
            Command::Add(v) => format!("add {}", v),
            Command::Del(i) => format!("del {}", i),
            Command::Set(i) => format!("set {}", i),
            Command::Tag(i, v) => format!("tag {} {}", i, v),
            Command::Show(i) => format!("show {}", i),
            Command::Load(v) => format!("load {}", v),
        };

        s.as_bytes().to_vec()
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
    let cmd = match payload.parse::<Command>() {
        Err(CommandParseError::EmptyCommand) => Command::Quit,
        Ok(cmd) => cmd,
        Err(err) => Command::Invalid(err),
    };

    Ok(cmd)
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
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: Some(String::from("127.0.0.1")),
            port: Some(8931),
        }
    }
}

pub fn load_config(filename: &str) -> io::Result<Config> {
    let mut file = File::open(filename)?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;

    let config: Config = toml::from_str(buffer.as_str())?;

    Ok(config)
}
