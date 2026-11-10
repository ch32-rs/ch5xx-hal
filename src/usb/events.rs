//! Pure interrupt event ordering decisions.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Transfer,
    Acknowledge,
    Setup,
}

// Reset consumes a simultaneous transfer; suspend and overflow remain independent.
pub(super) fn handle_transfer(transfer: bool, reset: bool) -> bool {
    transfer && !reset
}

// The token field can retain an earlier transaction when SETUP is reported.
pub(super) fn transfer_events(setup: bool, idle: bool) -> [Option<Event>; 4] {
    [
        Some(Event::Transfer),
        if setup && !idle { Some(Event::Acknowledge) } else { None },
        if setup { Some(Event::Setup) } else { None },
        Some(Event::Acknowledge),
    ]
}
