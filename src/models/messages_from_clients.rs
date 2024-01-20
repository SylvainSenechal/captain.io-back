// todo : rename "command"
#[derive(Debug)]
pub enum Command {
    JoinLobby(usize),
    SendGlobalMessage(String),
    SendLobbyMessage(usize, String),
    Ping,
}

impl std::str::FromStr for Command {
    type Err = ();
    fn from_str(msg: &str) -> Result<Command, Self::Err> {
        let commands: Vec<&str> = msg.splitn(2, ' ').collect();
        if commands.len() < 2 {
            // todo : recheck this, not always==2
            return Err(());
        }
        match commands[0] {
            "/joinlobby" => Ok(Command::JoinLobby(
                commands[1]
                    .parse::<usize>()
                    .expect("couldnt parse lobby id string to usize"),
            )),
            "/ping" => Ok(Command::Ping),
            "/sendGlobalMessage" => {
                println!("new global message {}", commands[1]);
                Ok(Command::SendGlobalMessage(commands[1].to_string()))
            }
            "/sendLobbyMessage" => {
                println!("new lobby message {}", commands[1]);
                let message: Vec<&str> = commands[1].splitn(2, ' ').collect(); // todo : check out of bound
                Ok(Command::SendLobbyMessage(
                    message[0]
                        .parse()
                        .expect("failed to parse usize for send message"),
                    message[1].to_string(),
                ))
            }
            _ => Err(()),
        }
    }
}
