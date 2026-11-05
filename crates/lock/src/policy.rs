//! Authentication success is the only event that can authorize an unlock.
use crate::auth::Error;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Acquiring,
    Locked,
    Authenticating(u64),
    Authenticated,
    Denied,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Confirmed,
    Denied,
    Submit,
    Authentication(u64, Result<(), Error>),
    Fatal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    Ready,
    Authenticate(u64),
    Unlock,
    ExitLocked,
}
pub struct Policy {
    pub state: State,
    serial: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            state: State::Acquiring,
            serial: 0,
        }
    }
}
impl Policy {
    pub fn update(&mut self, event: Event) -> Effect {
        match (self.state, event) {
            (State::Acquiring, Event::Confirmed) => {
                self.state = State::Locked;
                Effect::Ready
            }
            (State::Acquiring, Event::Denied) => {
                self.state = State::Denied;
                Effect::ExitLocked
            }
            (State::Locked, Event::Submit) => {
                self.serial += 1;
                self.state = State::Authenticating(self.serial);
                Effect::Authenticate(self.serial)
            }
            (State::Authenticating(active), Event::Authentication(id, result)) if active == id => {
                if result.is_ok() {
                    self.state = State::Authenticated;
                    Effect::Unlock
                } else {
                    self.state = State::Locked;
                    Effect::None
                }
            }
            (_, Event::Fatal) => {
                self.state = State::Failed;
                Effect::ExitLocked
            }
            _ => Effect::None,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_and_matching_success_are_required_for_unlock() {
        let mut p = Policy::default();
        assert_eq!(p.update(Event::Submit), Effect::None);
        assert_eq!(p.update(Event::Authentication(1, Ok(()))), Effect::None);
        assert_eq!(p.update(Event::Confirmed), Effect::Ready);
        assert_eq!(p.update(Event::Submit), Effect::Authenticate(1));
        assert_eq!(p.update(Event::Submit), Effect::None);
        assert_eq!(p.update(Event::Authentication(2, Ok(()))), Effect::None);
        assert_eq!(
            p.update(Event::Authentication(1, Err(Error::Denied))),
            Effect::None
        );
        assert_eq!(p.state, State::Locked);
        assert_eq!(p.update(Event::Submit), Effect::Authenticate(2));
        assert_eq!(p.update(Event::Authentication(1, Ok(()))), Effect::None);
        assert_eq!(p.update(Event::Authentication(2, Ok(()))), Effect::Unlock);
    }
    #[test]
    fn faults_never_unlock() {
        for events in [
            vec![Event::Fatal],
            vec![Event::Confirmed, Event::Fatal],
            vec![Event::Confirmed, Event::Submit, Event::Fatal],
            vec![Event::Denied],
        ] {
            let mut p = Policy::default();
            for e in events {
                assert_ne!(p.update(e), Effect::Unlock);
            }
        }
    }
}
