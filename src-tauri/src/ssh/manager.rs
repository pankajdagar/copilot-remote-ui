use std::process::Command;

use crate::sessions::model::Host;
use crate::ssh::openssh;

pub fn command(host: &Host, args: &[String]) -> Command {
    match &host.ssh_host {
        Some(alias) => openssh::command(alias, args),
        None => {
            let mut process = Command::new(&args[0]);
            process.args(&args[1..]);
            process
        }
    }
}
