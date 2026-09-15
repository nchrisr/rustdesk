//! Rules the device enforces itself because only it knows, in real time, who
//! is connected (plan §3.5). Everything else is the backend's decision.

use super::types::{ConnectedPeer, Role};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalDeny {
    /// A `user` may not join while another `user` holds a real (non-monitoring)
    /// connection. Admins and managers never block, and are never blocked.
    UserAlreadyConnected,
}

impl LocalDeny {
    pub fn reason_code(&self) -> &'static str {
        match self {
            LocalDeny::UserAlreadyConnected => "user_already_connected",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            LocalDeny::UserAlreadyConnected => {
                "Another user is currently connected to this device."
            }
        }
    }
}

pub fn check_local_rules(
    role: Role,
    monitoring: bool,
    connected: &[ConnectedPeer],
) -> Result<(), LocalDeny> {
    if role != Role::User || monitoring {
        return Ok(());
    }
    let another_user = connected
        .iter()
        .any(|c| c.role == Role::User && !c.monitoring);
    if another_user {
        Err(LocalDeny::UserAlreadyConnected)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(role: Role, monitoring: bool) -> ConnectedPeer {
        ConnectedPeer {
            peer_id: "p".into(),
            role,
            session_id: "s".into(),
            monitoring,
        }
    }

    #[test]
    fn user_alone_is_fine() {
        assert_eq!(check_local_rules(Role::User, false, &[]), Ok(()));
    }

    #[test]
    fn user_blocked_by_another_user() {
        assert_eq!(
            check_local_rules(Role::User, false, &[peer(Role::User, false)]),
            Err(LocalDeny::UserAlreadyConnected)
        );
    }

    #[test]
    fn user_not_blocked_by_admin_or_manager() {
        let present = [peer(Role::Admin, false), peer(Role::Manager, false)];
        assert_eq!(check_local_rules(Role::User, false, &present), Ok(()));
    }

    #[test]
    fn monitoring_connections_neither_block_nor_are_blocked() {
        // An admin watching in view-only mode is not "a user connected".
        assert_eq!(check_local_rules(Role::User, false, &[peer(Role::User, true)]), Ok(()));
        // And a monitoring connection itself is never subject to the rule.
        assert_eq!(check_local_rules(Role::User, true, &[peer(Role::User, false)]), Ok(()));
    }

    #[test]
    fn admin_and_manager_are_never_blocked() {
        let present = [peer(Role::User, false), peer(Role::User, false)];
        assert_eq!(check_local_rules(Role::Admin, false, &present), Ok(()));
        assert_eq!(check_local_rules(Role::Manager, false, &present), Ok(()));
    }
}
