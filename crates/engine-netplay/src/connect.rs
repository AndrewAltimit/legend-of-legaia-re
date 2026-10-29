//! The Multiplayer screen before a room exists: host one on a chosen port,
//! or join one by address, room code and name.
//!
//! The menu is a model with no sockets: a host steps it with the pad and
//! typed text and acts on the [`ConnectAction`] it returns (the native
//! window opens a relay or a connection; a browser page, which cannot
//! listen, offers only the join half). It draws through the same room
//! screen composition as a room ([`crate::screens`]).

use legaia_engine_system::pad::PadButton;
use legaia_engine_ui::expansion_modes::netplay::NetMenuRow;
use legaia_netplay::DEFAULT_PORT;
use legaia_netplay::protocol::{MAX_NAME_LEN, RoomCode};

/// A typed field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Port,
    Address,
    Room,
    Name,
}

/// The menu's rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    HostPort,
    Host,
    Address,
    Room,
    Name,
    Join,
    Close,
}

/// What a step asks of the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectAction {
    Host {
        port: u16,
        name: String,
    },
    Join {
        addr: String,
        room: RoomCode,
        name: String,
    },
    Close,
}

/// The Multiplayer screen's state.
#[derive(Debug, Clone)]
pub struct ConnectMenu {
    /// This host can listen (the native window; not a browser page).
    pub can_host: bool,
    pub port: u16,
    pub address: String,
    pub room: String,
    pub name: String,
    cursor: usize,
    /// The field being typed into, and its text so far.
    pub editing: Option<(Field, String)>,
    /// Why the last attempt failed.
    pub notice: Option<String>,
}

impl ConnectMenu {
    pub fn new(can_host: bool, name: &str) -> Self {
        Self {
            can_host,
            port: DEFAULT_PORT,
            address: String::new(),
            room: String::new(),
            name: name.to_string(),
            cursor: 0,
            editing: None,
            notice: None,
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        if self.can_host {
            rows.extend([Row::HostPort, Row::Host]);
        }
        rows.extend([Row::Address, Row::Room, Row::Name, Row::Join, Row::Close]);
        rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// One frame of pad edges. Ignored while a field is being typed (the
    /// host routes keys to [`Self::type_text`] then).
    pub fn step(&mut self, pressed: u16) -> Option<ConnectAction> {
        if self.editing.is_some() {
            return None;
        }
        let hit = |b: PadButton| pressed & b.mask() != 0;
        let rows = self.rows();
        self.cursor = self.cursor.min(rows.len() - 1);
        if hit(PadButton::Circle) || hit(PadButton::Triangle) {
            return Some(ConnectAction::Close);
        }
        if hit(PadButton::Up) {
            self.cursor = (self.cursor + rows.len() - 1) % rows.len();
        }
        if hit(PadButton::Down) {
            self.cursor = (self.cursor + 1) % rows.len();
        }
        let row = rows[self.cursor];
        if row == Row::HostPort {
            if hit(PadButton::Left) {
                self.port = self.port.saturating_sub(1).max(1);
            }
            if hit(PadButton::Right) {
                self.port = self.port.saturating_add(1);
            }
        }
        if !hit(PadButton::Cross) {
            return None;
        }
        let edit = |f: Field, v: &str| Some((f, v.to_string()));
        match row {
            Row::HostPort => self.editing = edit(Field::Port, &self.port.to_string()),
            Row::Address => self.editing = edit(Field::Address, &self.address),
            Row::Room => self.editing = edit(Field::Room, &self.room),
            Row::Name => self.editing = edit(Field::Name, &self.name),
            Row::Host => {
                return Some(ConnectAction::Host {
                    port: self.port,
                    name: self.player_name(),
                });
            }
            Row::Join => return self.join(),
            Row::Close => return Some(ConnectAction::Close),
        }
        None
    }

    fn player_name(&self) -> String {
        match self.name.trim() {
            "" => "Player".to_string(),
            n => n.to_string(),
        }
    }

    fn join(&mut self) -> Option<ConnectAction> {
        if self.address.trim().is_empty() {
            self.notice = Some("Type the host's address first".into());
            return None;
        }
        let Some(room) = RoomCode::parse(&self.room) else {
            self.notice = Some("A room code is six letters and digits".into());
            return None;
        };
        let addr = match self.address.trim() {
            a if a.contains(':') => a.to_string(),
            a => format!("{a}:{DEFAULT_PORT}"),
        };
        Some(ConnectAction::Join {
            addr,
            room,
            name: self.player_name(),
        })
    }

    /// Typed characters into the field being edited.
    pub fn type_text(&mut self, text: &str) {
        let Some((field, value)) = self.editing.as_mut() else {
            return;
        };
        for c in text.chars().filter(|c| !c.is_control()) {
            let ok = match field {
                Field::Port => c.is_ascii_digit() && value.len() < 5,
                Field::Room => c.is_ascii_alphanumeric() && value.len() < 6,
                Field::Address => !c.is_whitespace() && value.len() < 64,
                Field::Name => value.len() + c.len_utf8() <= MAX_NAME_LEN,
            };
            if ok {
                value.push(if *field == Field::Room {
                    c.to_ascii_uppercase()
                } else {
                    c
                });
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some((_, v)) = self.editing.as_mut() {
            v.pop();
        }
    }

    /// Keep the typed value.
    pub fn commit(&mut self) {
        let Some((field, v)) = self.editing.take() else {
            return;
        };
        match field {
            Field::Port => match v.parse::<u16>() {
                Ok(p) if p > 0 => self.port = p,
                _ => self.notice = Some("A port is a number from 1 to 65535".into()),
            },
            Field::Address => self.address = v,
            Field::Room => self.room = v,
            Field::Name => self.name = v,
        }
    }

    /// Drop the typed value.
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// The menu as room-screen rows.
    pub fn menu_rows(&self) -> Vec<NetMenuRow> {
        let row = |label: &str, value: Option<String>| NetMenuRow {
            label: label.to_string(),
            value,
            enabled: true,
        };
        let show = |v: &str, empty: &str| {
            Some(if v.is_empty() {
                empty.to_string()
            } else {
                v.to_string()
            })
        };
        self.rows()
            .into_iter()
            .map(|r| match r {
                Row::HostPort => row("Host on port", Some(self.port.to_string())),
                Row::Host => row("Host a room", None),
                Row::Address => row("Address", show(&self.address, "type...")),
                Row::Room => row("Room code", show(&self.room, "type...")),
                Row::Name => row("Name", show(&self.name, "type...")),
                Row::Join => row("Join", None),
                Row::Close => row("Close", None),
            })
            .collect()
    }

    /// The hint lines the screen shows where a room shows its chat.
    pub fn hints(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.can_host {
            out.push("Host: others join at this machine's address.".to_string());
        }
        out.push("Join: the host's address and room code.".to_string());
        out.push("Cross on a field types into it; Enter keeps it.".to_string());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(m: &mut ConnectMenu, b: PadButton) -> Option<ConnectAction> {
        m.step(b.mask())
    }

    #[test]
    fn join_takes_the_typed_address_code_and_name() {
        let mut m = ConnectMenu::new(true, "Vahn");
        for _ in 0..2 {
            press(&mut m, PadButton::Down);
        }
        press(&mut m, PadButton::Cross);
        m.type_text("192.168.1.4");
        m.commit();
        press(&mut m, PadButton::Down);
        press(&mut m, PadButton::Cross);
        m.type_text("k7qx2m!");
        m.commit();
        // Joining needs no name change; Down twice reaches Join.
        press(&mut m, PadButton::Down);
        press(&mut m, PadButton::Down);
        assert_eq!(
            press(&mut m, PadButton::Cross),
            Some(ConnectAction::Join {
                addr: "192.168.1.4:7462".into(),
                room: RoomCode::parse("K7QX2M").unwrap(),
                name: "Vahn".into(),
            })
        );
    }

    #[test]
    fn host_takes_the_port_and_a_browser_offers_only_join() {
        let mut m = ConnectMenu::new(true, "");
        press(&mut m, PadButton::Right);
        press(&mut m, PadButton::Down);
        assert_eq!(
            press(&mut m, PadButton::Cross),
            Some(ConnectAction::Host {
                port: DEFAULT_PORT + 1,
                name: "Player".into()
            })
        );
        let web = ConnectMenu::new(false, "");
        assert!(web.menu_rows().iter().all(|r| !r.label.starts_with("Host")));
        let mut m = ConnectMenu::new(false, "");
        for _ in 0..3 {
            press(&mut m, PadButton::Down);
        }
        assert_eq!(press(&mut m, PadButton::Cross), None);
        assert!(m.notice.is_some(), "no address yet");
    }
}
