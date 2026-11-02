//! The kinds of in-app notification (docs/messaging.md §2.1), shared by the
//! server that writes them and the interface that words them.

use serde::{Deserialize, Serialize};

/// What a notification is about (docs/messaging.md §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// A membership was created for the reader.
    TreeShared,
    /// The reader's role, scope or expiry changed.
    MembershipChanged,
    /// The reader was removed from a tree, or their membership expired.
    MembershipEnded,
    /// An invitation of the reader's tree became a membership.
    InvitationAccepted,
    /// A tree changed owner: to the new and the former owner.
    OwnershipTransferred,
    /// A member left the reader's tree.
    MemberLeft,
    /// An import or export the reader started completed or failed.
    JobFinished,
}

impl NotificationKind {
    pub const ALL: [Self; 7] = [
        Self::TreeShared,
        Self::MembershipChanged,
        Self::MembershipEnded,
        Self::InvitationAccepted,
        Self::OwnershipTransferred,
        Self::MemberLeft,
        Self::JobFinished,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TreeShared => "tree_shared",
            Self::MembershipChanged => "membership_changed",
            Self::MembershipEnded => "membership_ended",
            Self::InvitationAccepted => "invitation_accepted",
            Self::OwnershipTransferred => "ownership_transferred",
            Self::MemberLeft => "member_left",
            Self::JobFinished => "job_finished",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_serializes_as_its_stored_value() {
        for kind in NotificationKind::ALL {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::from(kind.as_str())
            );
            assert_eq!(NotificationKind::parse(kind.as_str()), Some(kind));
        }
    }
}
