use std::collections::HashMap;
use crate::room_data::{RoomData, Rooms};
use common::{
    room_state::{configuration::*, member::*, member_info::*, message::*},
    ChatRoomStateV1,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::time::{Duration, UNIX_EPOCH};
use dioxus_logger::tracing::info;
use common::room_state::ChatRoomParametersV1;
use freenet_scaffold::ComposableState;
use lipsum::lipsum;

pub fn create_example_rooms() -> Rooms {
    let mut map = HashMap::new();
    let mut csprng = OsRng;

    // Room where you're just an observer (not a member)
    let (owner_vk_1, _, room_data_1) = create_room(
        &mut csprng,
        "PublicRoomOwner",
        vec![],
        &"Public Discussion Room".to_string()
    );
    map.insert(owner_vk_1, room_data_1);

    // Room where you're a member
    let (owner_vk_2, _, room_data_2) = create_room(
        &mut csprng,
        "TeamLead",
        vec!["You", "Colleague1", "Colleague2"],
        &"Team Chat Room".to_string()
    );
    map.insert(owner_vk_2, room_data_2);

    // Room where you're the owner
    let (owner_vk_3, _, room_data_3) = create_room(
        &mut csprng,
        "You",
        vec!["Member1", "Member2"],
        &"Your Private Room".to_string()
    );
    map.insert(owner_vk_3, room_data_3);

    Rooms { map }
}

// Function to create a room with an owner and members
fn create_room(csprng: &mut OsRng, owner_name: &str, member_names: Vec<&str>, room_name : &String) -> (VerifyingKey, Option<VerifyingKey>, RoomData) {
    let owner_key = SigningKey::generate(csprng);
    let owner_vk = owner_key.verifying_key();
    let owner_id = MemberId::new(&owner_vk);
    info!("{}'s owner ID: {}", owner_name, owner_id);

    let mut room_state = ChatRoomStateV1::default();

    // Set configuration
    let mut config = Configuration::default();
    config.name = room_name.clone();
    config.owner_member_id = owner_id;
    room_state.configuration = AuthorizedConfigurationV1::new(config, &owner_key);

    // Add members
    let mut members = MembersV1::default();
    let mut member_info = MemberInfoV1::default();
    let mut member_vk = None;
    let mut your_member_key = None;

    // Add owner first
    add_member(&mut members, &mut member_info, owner_name, &owner_key, &owner_id, &owner_key);
    if owner_name == "You" {
        your_member_key = Some(owner_key.clone());
    }

    // Add other members
    for &name in &member_names {
        let member_signing_key = SigningKey::generate(csprng);
        let member_vk_temp = member_signing_key.verifying_key();
        let member_id = MemberId::new(&member_vk_temp);
        info!("{}'s member ID: {}", name, member_id);

        if name == "You" {
            your_member_key = Some(member_signing_key.clone());
        }

        add_member(&mut members, &mut member_info, name, &owner_key, &member_id, &member_signing_key);
        member_vk = Some(member_vk_temp);
    }

    room_state.members = members;
    room_state.member_info = member_info;

    // Create a HashMap of member keys including the owner
    let mut member_keys = HashMap::new();
    member_keys.insert(owner_id, owner_key.clone());
    if let Some(ref key) = your_member_key {
        member_keys.insert(MemberId::new(&key.verifying_key()), key.clone());
    }
    
    // Add example messages if there are any members
    if !member_keys.is_empty() {
        add_example_messages(
            &mut room_state,
            &owner_vk,
            &member_keys,
        );
    }

    let user_signing_key = if owner_name == "You" {
        // If you're the owner, use the owner key
        owner_key
    } else if let Some(key) = your_member_key {
        // If you're a member, use your member key
        key
    } else {
        // Otherwise generate a new key for an observer
        SigningKey::generate(csprng)
    };

    let verification_result = room_state.verify(&room_state, &ChatRoomParametersV1 { owner: owner_vk });
    if !verification_result.is_ok() {
        panic!("Failed to verify room state: {:?}", verification_result.err());
    }

    (
        owner_vk,
        member_vk,
        RoomData {
            room_state,
            user_signing_key,
        },
    )
}

// Function to add a member to the room
fn add_member(
    members: &mut MembersV1,
    member_info: &mut MemberInfoV1,
    name: &str,
    owner_key: &SigningKey,
    member_id: &MemberId,
    signing_key: &SigningKey,
) {
    let member_vk = signing_key.verifying_key();
    let owner_member_id = MemberId::new(&owner_key.verifying_key());
    
    // Only add non-owner members to the members list
    if member_id != &owner_member_id {
        members.members.push(AuthorizedMember::new(
            Member {
                owner_member_id,
                invited_by: owner_member_id,  // Owner invites all members
                member_vk: member_vk.clone(),
            },
            owner_key,
        ));
    }
    // Add member info for both owner and regular members
    member_info.member_info.push(AuthorizedMemberInfo::new_with_member_key(
        MemberInfo {
            member_id: *member_id,
            version: 0,
            preferred_nickname: name.to_string(),
        },
        signing_key,
    ));
}

fn add_example_messages(
    room_state: &mut ChatRoomStateV1,
    owner_vk: &VerifyingKey,
    member_keys: &HashMap<MemberId, SigningKey>,
) {

    let base_time = UNIX_EPOCH + Duration::from_secs(1633012200); // September 30, 2021 14:30:00 UTC
    let mut messages = MessagesV1::default();
    let owner_id = MemberId::new(owner_vk);
    let mut current_time = base_time;

    // As a sanity check, verify that all member_keys are valid and members exist
    for (member_id, signing_key) in member_keys.iter() {
        if MemberId::new(&signing_key.verifying_key()) != *member_id {
            panic!("Member ID does not match signing key");
        }

        // For non-owner members, verify they exist in both members list and member_info
        if member_id != &owner_id {
            if !room_state.members.members.iter().any(|m| m.member.id() == *member_id) {
                panic!("Member ID not found in members list: {}", member_id);
            }
            if !room_state.member_info.member_info.iter().any(|m| m.member_info.member_id == *member_id) {
                panic!("Member ID not found in member_info: {}", member_id);
            }
        }
    }

    // First verify we have the owner's key and info
    if !member_keys.contains_key(&owner_id) || 
       !room_state.member_info.member_info.iter().any(|m| m.member_info.member_id == owner_id) {
        return; // Can't add messages without owner key and info
    }

    // Add owner's messages first
    if let Some(owner_key) = member_keys.get(&owner_id) {
        messages.messages.push(AuthorizedMessageV1::new(
            MessageV1 {
                room_owner: owner_id,
                author: owner_id,
                time: current_time,
                content: lipsum(20),
            },
            owner_key,
        ));
        current_time += Duration::from_secs(60);

        messages.messages.push(AuthorizedMessageV1::new(
            MessageV1 {
                room_owner: owner_id,
                author: owner_id,
                time: current_time,
                content: lipsum(15),
            },
            owner_key,
        ));
        current_time += Duration::from_secs(60);
    }

    // Generate two messages for each non-owner member
    for (member_id, signing_key) in member_keys.iter() {
        // Skip owner as we already handled them
        if *member_id == owner_id {
            continue;
        }

        // Skip if this member doesn't exist in both members list and member_info
        if !room_state.members.members.iter().any(|m| m.member.id() == *member_id) ||
           !room_state.member_info.member_info.iter().any(|m| m.member_info.member_id == *member_id) {
            info!("Skipping messages for member {} as they are not fully registered", member_id);
            continue;
        }

        // First message
        messages.messages.push(AuthorizedMessageV1::new(
            MessageV1 {
                room_owner: owner_id,
                author: *member_id,
                time: current_time,
                content: lipsum(20),
            },
            signing_key,
        ));
        current_time += Duration::from_secs(60);

        // Second message
        messages.messages.push(AuthorizedMessageV1::new(
            MessageV1 {
                room_owner: owner_id,
                author: *member_id,
                time: current_time,
                content: lipsum(15),
            },
            signing_key,
        ));
        current_time += Duration::from_secs(60);
    }

    room_state.recent_messages = messages;
}

// Test function to create the example data
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_example_rooms() {
        let rooms = create_example_rooms();
        assert_eq!(rooms.map.len(), 3);
    }
}
