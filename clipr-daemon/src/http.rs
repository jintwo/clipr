use anyhow::Result;
use socket2::{Domain, Socket, Type};
use std::io::{BufReader, prelude::*};
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

// fn client(listen_on: &str, cmd: Command) -> Result<Payload> {
//     let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
//     socket.set_reuse_port(true)?;
//     socket.set_reuse_address(true)?;
//     let address: SocketAddr = listen_on.parse()?;
//     socket.connect(&address.into())?;
//     let stream: TcpStream = socket.into();
//     let cmd_body = serde_json::to_string(&cmd)?;
//     stream.write_all("POST /command HTTP/1.1\r\n\r\n".as_bytes())?;
//     stream.write_all(cmd_body.as_bytes())?;
//     stream.flush()?;
//     // stream.read_to_string
//     // TODO:
//     // +1. serialize command
//     // +2. write data
//     // +3. flush
//     // 4. read response
//     // 5. deserialize response
// }

fn handle_connection(sender: Sender<clipr_common::Request>, mut stream: &TcpStream) -> Result<()> {
    let mut buf_reader = BufReader::new(&mut stream);
    // TODO: fix it
    let mut buffer = [0; 1024];
    let size = buf_reader.read(&mut buffer)?;
    let mut headers = [httparse::EMPTY_HEADER; 16];
    let mut req = httparse::Request::new(&mut headers);
    let offset = req.parse(&buffer[..size])?.unwrap();
    // TODO: add content-length assertion
    let _content_length_pre = size - offset;
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
            // TODO: content_length_pre == content_length
            let body = &buffer[offset..];
            let cmd: clipr_common::Command = serde_json::from_slice(&body[..content_length])?;
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
