use anyhow::Result;
use socket2::{Domain, Socket, Type};
use std::io::{prelude::*, BufReader};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::Sender;

pub fn server(listen_on: String, sender: Sender<clipr_common::Request>) -> Result<()> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    socket.set_reuse_port(true)?;
    socket.set_reuse_address(true)?;
    let address: SocketAddr = listen_on.parse()?;
    socket.bind(&address.into())?;
    socket.listen(10)?;
    let listener: TcpListener = socket.into();
    for stream in listener.incoming() {
        let stream = stream?;
        handle_connection(sender.clone(), &stream)?;
    }
    Ok(())
}

fn handle_connection(sender: Sender<clipr_common::Request>, mut stream: &TcpStream) -> Result<()> {
    let mut buffer = [0; 256];
    let mut buf_reader = BufReader::new(&mut stream);
    let size = buf_reader.read(&mut buffer)?;
    let mut headers = [httparse::EMPTY_HEADER; 4];
    let mut req = httparse::Request::new(&mut headers);
    let offset = req.parse(&buffer[..size])?.unwrap();
    let response = match (req.method, req.path) {
        (Some("POST"), Some("/command")) => {
            let content_length: usize = String::from_utf8_lossy(
                req.headers
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
            let cmd: clipr_common::Command = serde_json::from_slice(&body)?;
            let rep_body = match clipr_common::Request::send_cmd(&sender, cmd) {
                Some(clipr_common::Response::Payload(val)) => serde_json::to_string(&val)?,
                _ => String::from("{}"),
            };
            let length = rep_body.len();
            format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\n\r\n{rep_body}")
        }
        _ => String::from("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n"),
    };
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}
