//! `legaia-relay`: the standalone netplay relay. Rooms, lobby and message
//! routing for raw-TCP and WebSocket clients on one port.
//!
//! The relay needs sockets, so on `wasm32` the binary is an empty stub; it
//! still builds, so a wasm check with the default features passes.

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    native::main()
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::process::ExitCode;

    use clap::Parser;
    use legaia_netplay::DEFAULT_PORT;
    use legaia_netplay::relay::RelayLimits;
    use legaia_netplay::relay::server::{RelayServer, ServerConfig};

    /// Relay server for Legend of Legaia netplay rooms.
    ///
    /// Room codes are the only access control: run it on a LAN or for
    /// friends, and put a TLS-terminating proxy in front of it for browser
    /// clients on an https page.
    #[derive(Parser)]
    #[command(version)]
    struct Args {
        /// Address to listen on.
        #[arg(long, default_value_t = format!("0.0.0.0:{DEFAULT_PORT}"))]
        bind: String,
        /// Most simultaneous connections.
        #[arg(long, default_value_t = RelayLimits::default().max_connections)]
        max_connections: usize,
        /// Most simultaneous rooms.
        #[arg(long, default_value_t = RelayLimits::default().max_rooms)]
        max_rooms: usize,
        /// Most connections from one IP address.
        #[arg(long, default_value_t = ServerConfig::default().max_connections_per_ip)]
        max_per_ip: usize,
    }

    pub fn main() -> ExitCode {
        let args = Args::parse();
        let config = ServerConfig {
            limits: RelayLimits {
                max_connections: args.max_connections,
                max_rooms: args.max_rooms,
                ..RelayLimits::default()
            },
            max_connections_per_ip: args.max_per_ip,
        };
        match RelayServer::spawn_with(args.bind.as_str(), config) {
            Ok(handle) => {
                eprintln!("legaia-relay listening on {}", handle.local_addr());
                handle.wait();
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("legaia-relay: cannot listen on {}: {e}", args.bind);
                ExitCode::FAILURE
            }
        }
    }
}
