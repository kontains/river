//! Handles pending room invitations and join requests
//!
//! This module manages the state of room invitations that are in the process
//! of being accepted or retrieved.

use ed25519_dalek::{SigningKey, VerifyingKey};
use river_common::room_state::member::AuthorizedMember;
use std::collections::HashMap;

/// Collection of pending room join requests
#[derive(Clone, Debug, Default)]
pub struct PendingInvites {
    /// Map of room owner keys to pending join information
    pub map: HashMap<VerifyingKey, PendingRoomJoin>,
}

/// Information about a pending room join
#[derive(Clone, Debug)]
pub struct PendingRoomJoin {
    /// The authorized member data for the join
    pub authorized_member: AuthorizedMember,
    /// The signing key for the invited member
    pub invitee_signing_key: SigningKey,
    /// User's preferred nickname for this room
    pub preferred_nickname: String,
    /// Current status of the join request
    pub status: PendingRoomStatus,
}

/// Status of a pending room join request
#[derive(Clone, Debug, PartialEq)]
pub enum PendingRoomStatus {
    /// Currently retrieving room data
    Retrieving,
    /// Successfully retrieved room data
    Retrieved,
    /// Error occurred during retrieval
    Error(String),
}
