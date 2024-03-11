use crate::service_layer::player_service::PlayerMove;

#[derive(Debug)]
pub enum ClientCommand {
    Move(PlayerMove),
    JoinLobby(usize),
    SendGlobalMessage(String),
    SendLobbyMessage(String),
    Ping,
}

impl std::str::FromStr for ClientCommand {
    type Err = ();
    fn from_str(msg: &str) -> Result<ClientCommand, Self::Err> {
        let mut commands = msg.splitn(2, ' ');

        if let Some(command_type) = commands.next() {
            match command_type {
                "/move" => match commands.next().ok_or(())?.parse::<PlayerMove>() {
                    Ok(new_move) => Ok(ClientCommand::Move(new_move)),
                    Err(_) => Err(()),
                },
                "/joinLobby" => match commands.next().ok_or(())?.parse::<usize>() {
                    Ok(lob) => Ok(ClientCommand::JoinLobby(lob)),
                    Err(_) => Err(()),
                },
                "/ping" => Ok(ClientCommand::Ping),
                "/sendGlobalMessage" => {
                    let new_message = commands.next().ok_or(())?;
                    Ok(ClientCommand::SendGlobalMessage(new_message.to_string()))
                }
                "/sendLobbyMessage" => {
                    let new_message = commands.next().ok_or(())?;
                    Ok(ClientCommand::SendLobbyMessage(new_message.to_string()))
                }
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}
