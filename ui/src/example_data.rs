use std::collections::HashMap;
use crate::room_data::{RoomData, Rooms};
use common::{
    room_state::{configuration::*, member::*, member_info::*, message::*},
    ChatRoomStateV1,
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use std::time::{Duration, UNIX_EPOCH};
use common::room_state::ChatRoomParametersV1;
use freenet_scaffold::ComposableState;
use lipsum::lipsum;

pub fn create_example_rooms() -> Rooms {
    let mut map = HashMap::new();
    let mut csprng = OsRng;
/*
    // Room where you're just an observer (not a member)
    let (owner_vk_1, room_data_1) = create_room(
        &mut csprng,
        &"Public Discussion Room".to_string(),
    );
    map.insert(owner_vk_1, room_data_1);

    // Room where you're a member
    let (owner_vk_2, room_data_2) = create_room(
        &mut csprng,
        &"Team Chat Room".to_string(),
    );
    map.insert(owner_vk_2, room_data_2);

    // Room where you're the owner
    let (owner_vk_3, room_data_3) = create_room(
        &mut csprng,
        &"Your Private Room".to_string(),
    );
    map.insert(owner_vk_3, room_data_3);
*/
    Rooms { map }
}

struct CreatedRoom {
    owner_vk: VerifyingKey,
    room_data: RoomData,
    self_sk: SigningKey,
}

#[derive(Debug, PartialEq)]
enum SelfIs {
    Observer,
    Member,
    Owner,
}

// Function to create a room with an owner and members, self_is determines whether
// the user is the owner, a member, or an observer (not an owner or member)
fn create_room(
    room_name: &String,
    self_is: SelfIs,
) -> CreatedRoom {
    let mut csprng = OsRng;

    // Create self - the user actually using the app
    let self_sk = SigningKey::generate(&mut csprng);
    let self_vk = self_sk.verifying_key();
    let self_id = self_vk.into();

    // Create owner of the room
    let owner_sk = if self_is == SelfIs::Owner { &self_sk } else { &SigningKey::generate(&mut csprng) };
    let owner_vk = owner_sk.verifying_key();
    let owner_id = MemberId::from(&owner_vk);

    let mut room_state = ChatRoomStateV1::default();

    // Set configuration
    let mut config = Configuration::default();
    config.name = room_name.clone();
    config.owner_member_id = owner_id;
    room_state.configuration = AuthorizedConfigurationV1::new(config, &owner_sk);

    // Create a HashMap to store all member keys
    let mut member_keys : HashMap<MemberId, SigningKey> = HashMap::new();

    let mut members = MembersV1::default();
    let mut member_info = MemberInfoV1::default();

    // Add owner to member_info
    member_info.member_info.push(AuthorizedMemberInfo::new_with_member_key(
        MemberInfo {
            member_id: owner_id,
            version: 0,
            preferred_nickname: lipsum(2),
        },
        &owner_sk,
    ));

    // Self isn't the owner but is a member so must be added to members and member_info,
    // invited by owner
    if self_is == SelfIs::Member {
        // Add self to members, invited by owner
        members.members.push(AuthorizedMember::new(
            Member {
                owner_member_id: owner_id,
                invited_by: owner_id,
                member_vk: self_vk.clone(),
            },
            &owner_sk,
        ));

        // Add self to member_info
        member_info.member_info.push(AuthorizedMemberInfo::new_with_member_key(
            MemberInfo {
                member_id: self_id,
                version: 0,
                preferred_nickname: lipsum(2),
            },
            &self_sk,
        ));
    }

    // Variable to hold your signing key
    let mut your_signing_key = None;

    // Add members to the room

    room_state.members = members;
    room_state.member_info = member_info;

    // Add a user that's not the owner or self
    let user_sk = SigningKey::generate(&mut csprng);
    let user_vk = user_sk.verifying_key();
    let member_id = MemberId::from(&user_vk);



    let verification_result = room_state.verify(
        &room_state,
        &ChatRoomParametersV1 {
            owner: owner_vk.clone(),
        },
    );
    if !verification_result.is_ok() {
        panic!(
            "Failed to verify room state: {:?}",
            verification_result.err()
        );
    }

    CreatedRoom {
        owner_vk,
        room_data: RoomData {
            room_state,
            self_sk: user_sk, // TODO: Probably wrong
        },
        self_sk: your_signing_key.unwrap(),
    }
}

// Function to add a member to the room
fn add_member(
    members: &mut MembersV1,
    member_info: &mut MemberInfoV1,
    owner_key: &SigningKey,
    member_id: &MemberId,
    member_key: &SigningKey,
    invited_by: MemberId,
) {
    let member_vk = member_key.verifying_key();
    let owner_member_id = MemberId::from(&owner_key.verifying_key());

    // Add member to the members list
    members.members.push(AuthorizedMember::new(
        Member {
            owner_member_id,
            invited_by,
            member_vk: member_vk.clone(),
        },
        &owner_key,
    ));

    // Add member info
    member_info.member_info.push(AuthorizedMemberInfo::new_with_member_key(
        MemberInfo {
            member_id: *member_id,
            version: 0,
            preferred_nickname: lipsum(2),
        },
        member_key,
    ));
}

fn add_example_messages(
    room_state: &mut ChatRoomStateV1,
    owner_id: &MemberId,
    owner_key: &SigningKey,
    member_keys: &HashMap<MemberId, SigningKey>,
) {
    let base_time = UNIX_EPOCH + Duration::from_secs(1633012200); // September 30, 2021 14:30:00 UTC
    let mut messages = MessagesV1::default();
    let mut current_time = base_time;

    // Verify that all member_keys are valid and members exist
    for (member_id, signing_key) in member_keys.iter() {
        if MemberId::from(&signing_key.verifying_key()) != *member_id {
            panic!("Member ID does not match signing key");
        }

        // Verify they exist in members list
        if !room_state
            .members
            .members
            .iter()
            .any(|m| m.member.id() == *member_id)
        {
            panic!("Member ID not found in members list: {}", member_id);
        }
    }

    // Add messages from each member
    for (member_id, signing_key) in member_keys.iter() {
        // Add two messages for this member
        for i in 0..2 {
            messages.messages.push(AuthorizedMessageV1::new(
                MessageV1 {
                    room_owner: *owner_id,
                    author: *member_id,
                    time: current_time,
                    content: lipsum(if i == 0 { 20 } else { 15 }),
                },
                signing_key,
            ));
            current_time += Duration::from_secs(60);
        }
    }

    // Add messages from the owner
    messages.messages.push(AuthorizedMessageV1::new(
        MessageV1 {
            room_owner: *owner_id,
            author: *owner_id,
            time: current_time,
            content: lipsum(25),
        },
        owner_key,
    ));
    current_time += Duration::from_secs(60);

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

        for (owner_vk, room_data) in rooms.map.iter() {
            // Verify the room state
            let verification_result = room_data.room_state.verify(
                &room_data.room_state,
                &ChatRoomParametersV1 {
                    owner: owner_vk.clone(),
                },
            );
            assert!(
                verification_result.is_ok(),
                "Room state failed to verify: {:?}",
                verification_result.err()
            );

            // Check that the owner is not in the members list
            let owner_id = MemberId::from(&owner_vk);
            let owner_in_members = room_data
                .room_state
                .members
                .members
                .iter()
                .any(|m| m.member.id() == owner_id);
            assert!(
                !owner_in_members,
                "Owner should not be in the members list for room owned by {}",
                owner_id
            );

            // Check that messages are from valid members or the owner
            for authorized_message in room_data.room_state.recent_messages.messages.iter() {
                let message_author = authorized_message.message.author;
                let author_is_owner = message_author == owner_id;
                let author_in_members = room_data
                    .room_state
                    .members
                    .members
                    .iter()
                    .any(|m| m.member.id() == message_author);
                assert!(
                    author_is_owner || author_in_members,
                    "Message author {} is neither the owner nor in the members list",
                    message_author
                );
            }
        }
    }

    #[test]
    fn test_add_member() {
        let mut csprng = OsRng;
        let owner_key = SigningKey::generate(&mut csprng);
        let owner_id = MemberId::from(&owner_key.verifying_key());

        let member_key = SigningKey::generate(&mut csprng);
        let member_id = MemberId::from(&member_key.verifying_key());

        let mut members = MembersV1::default();
        let mut member_info = MemberInfoV1::default();

        add_member(
            &mut members,
            &mut member_info,
            &owner_key,
            &member_id,
            &member_key,
            owner_id,
        );

        assert_eq!(members.members.len(), 1);
        assert_eq!(member_info.member_info.len(), 1);

        // Verify that the member was added correctly
        let added_member = &members.members[0].member;
        assert_eq!(added_member.id(), member_id);
    }

    #[test]
    fn test_add_example_messages() {
        let mut csprng = OsRng;
        let owner_key = SigningKey::generate(&mut csprng);
        let owner_id = MemberId::from(&owner_key.verifying_key());

        let member_key = SigningKey::generate(&mut csprng);
        let member_id = MemberId::from(&member_key.verifying_key());

        let mut room_state = ChatRoomStateV1::default();

        // Add member to the room
        let mut members = MembersV1::default();
        let mut member_info = MemberInfoV1::default();
        add_member(
            &mut members,
            &mut member_info,
            &owner_key,
            &member_id,
            &member_key,
            owner_id,
        );
        room_state.members = members;
        room_state.member_info = member_info;

        let mut member_keys = HashMap::new();
        member_keys.insert(member_id, member_key);

        add_example_messages(&mut room_state, &owner_id, &owner_key, &member_keys);

        // Verify that messages are added
        assert_eq!(room_state.recent_messages.messages.len(), 3);

        // Verify that messages are from valid members or the owner
        for authorized_message in room_state.recent_messages.messages.iter() {
            let message_author = authorized_message.message.author;
            let author_is_owner = message_author == owner_id;
            let author_in_members = room_state
                .members
                .members
                .iter()
                .any(|m| m.member.id() == message_author);
            assert!(
                author_is_owner || author_in_members,
                "Message author {} is neither the owner nor in the members list",
                message_author
            );
        }
    }
}
