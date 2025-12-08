use miette::Result;
use rand::{Rng, distr::Alphanumeric};

use crate::state::StateManager;
const DEFAULT_POSTGRES_PORT: u16 = 5432;
const PORT_SEARCH_RANGE: u16 = 100;

pub fn find_available_port(state: &StateManager) -> Result<u16> {
    use std::net::TcpListener;

    let starting_port = state
        .get_highest_used_port()
        .unwrap_or(DEFAULT_POSTGRES_PORT);

    for port in starting_port..(starting_port + PORT_SEARCH_RANGE) {
        if TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return Ok(port);
        }
    }

    miette::bail!(
        "No available ports found in range {}-{}",
        DEFAULT_POSTGRES_PORT,
        DEFAULT_POSTGRES_PORT + PORT_SEARCH_RANGE - 1
    )
}

const PASSWORD_LENGTH: usize = 16;
pub fn generate_password() -> String {
    (&mut rand::rng())
        .sample_iter(Alphanumeric)
        .take(PASSWORD_LENGTH)
        .map(|b| b as char)
        .collect()
}
