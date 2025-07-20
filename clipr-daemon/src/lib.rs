use anyhow::Result;
use socket2::{Domain, Socket, Type};
use std::io::{prelude::*, BufReader};
use std::net::{SocketAddr, TcpStream};

use clipr_common::{Command, Config, Payload};

fn client(listen_on: &str, cmd: clipr_common::Command) -> Result<Payload> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    socket.set_reuse_port(true)?;
    socket.set_reuse_address(true)?;
    let address: SocketAddr = listen_on.parse()?;
    socket.connect(&address.into())?;
    let mut stream: TcpStream = socket.into();
    let cmd_body = serde_json::to_string(&cmd)?;
    let body = cmd_body.as_bytes();
    let body_len = cmd_body.len();
    stream.write_all(
        format!("POST /command HTTP/1.1\r\nContent-Length: {body_len}\r\n\r\n").as_bytes(),
    )?;
    stream.write_all(body)?;
    stream.flush()?;

    let mut buffer = [0; 256];
    let mut buf_reader = BufReader::new(&mut stream);
    let size = buf_reader.read(&mut buffer)?;
    let mut headers = [httparse::EMPTY_HEADER; 4];
    let mut rep = httparse::Response::new(&mut headers);
    let offset = rep.parse(&buffer[..size])?.unwrap();
    let content_length: usize = String::from_utf8_lossy(
        rep.headers
            .iter()
            .find(|&h| h.name.to_lowercase() == "content-length")
            .unwrap()
            .value,
    )
    .parse()?;
    let read_body_bytes = size - offset;
    let mut body: Vec<u8> = Vec::from(&buffer[offset..offset + read_body_bytes]);
    if read_body_bytes < content_length {
        let mut rest: Vec<u8> = vec![0; content_length - read_body_bytes];
        buf_reader.read_exact(&mut rest)?;
        body.append(&mut rest);
    }
    let payload: clipr_common::Payload = serde_json::from_slice(&body)?;
    Ok(payload)
}

pub fn call(config: &Config, cmd: Command) -> Result<Payload> {
    client(&config.listen_on(), cmd)
}
