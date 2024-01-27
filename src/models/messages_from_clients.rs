#[derive(Debug)]
pub enum ClientCommand {
    JoinLobby(usize),
    SendGlobalMessage(String),
    SendLobbyMessage(String),
    Ping,
}

// todo : handle expect errors
impl std::str::FromStr for ClientCommand {
    type Err = ();
    fn from_str(msg: &str) -> Result<ClientCommand, Self::Err> {
        // let commands: Vec<&str> = msg.splitn(2, ' ').collect();
        let mut commands = msg.splitn(2, ' ');
        // println!("commands: {:?}", commands);

        if let Some(command_type) = commands.next() {
            match command_type {
                "/joinLobby" => Ok(ClientCommand::JoinLobby(
                    commands
                        .next()
                        .expect("failed to fined en of command")
                        .parse::<usize>()
                        .expect("couldnt parse lobby id string to usize"),
                )),
                "/ping" => Ok(ClientCommand::Ping),
                "/sendGlobalMessage" => Ok(ClientCommand::SendGlobalMessage(
                    commands
                        .next()
                        .expect("failed to fined en of command")
                        .to_string(),
                )),
                "/sendLobbyMessage" => Ok(ClientCommand::SendLobbyMessage(
                    commands.next().expect("no lobby message found").to_string(),
                )),
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}
