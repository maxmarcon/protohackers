use super::msg;

#[derive(Debug, Clone)]
pub enum Event {
    Ticket(msg::Ticket, u16),     // ticket msg, dispatcher_id
    NewDispatcher(u16, Vec<u16>), // dispatcher_id, roads
}
