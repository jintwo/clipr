use anyhow::Result;
use std::time::Duration;

use clipr_common::{Command, Config, Payload};

pub fn call(config: &Config, cmd: Command) -> Result<Payload> {
    let uri = format!("http://{}/command", config.listen_on());
    let rep = reqwest::blocking::Client::new()
        .post(uri)
        .timeout(Duration::from_secs(2))
        .json(&cmd)
        .send()?
        .json::<Payload>()?;
    Ok(rep)
}
