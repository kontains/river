use crate::state::member::MemberId;
use crate::state::ChatRoomParametersV1;
use crate::util::sign_struct;
use crate::util::{truncated_base64, verify_struct};
use crate::ChatRoomStateV1;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use freenet_scaffold::util::{fast_hash, FastHash};
use freenet_scaffold::ComposableState;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct MessagesV1 {
    pub messages: Vec<AuthorizedMessageV1>,
}

impl ComposableState for MessagesV1 {
    type ParentState = ChatRoomStateV1;
    type Summary = Vec<MessageId>;
    type Delta = Vec<AuthorizedMessageV1>;
    type Parameters = ChatRoomParametersV1;

    fn verify(
        &self,
        parent_state: &Self::ParentState,
        _parameters: &Self::Parameters,
    ) -> Result<(), String> {
        let members_by_id = parent_state.members.members_by_member_id();

        for message in &self.messages {
            if let Some(member) = members_by_id.get(&message.message.author) {
                if message.validate(&member.member.member_vk).is_err() {
                    return Err(format!(
                        "Invalid message signature: id:{:?} content:{:?}",
                        message.id(),
                        message.message.content
                    ));
                }
            } else {
                return Err(format!(
                    "Message author not found: {:?}",
                    message.message.author
                ));
            }
        }

        Ok(())
    }

    fn summarize(
        &self,
        _parent_state: &Self::ParentState,
        _parameters: &Self::Parameters,
    ) -> Self::Summary {
        self.messages.iter().map(|m| m.id()).collect()
    }

    fn delta(
        &self,
        _parent_state: &Self::ParentState,
        _parameters: &Self::Parameters,
        old_state_summary: &Self::Summary,
    ) -> Option<Self::Delta> {
        let delta : Vec<AuthorizedMessageV1> = self.messages
            .iter()
            .filter(|m| !old_state_summary.contains(&m.id()))
            .cloned()
            .collect();
        if delta.is_empty() {
            None
        } else {
            Some(delta)
        }
    }

    fn apply_delta(
        &mut self,
        parent_state: &Self::ParentState,
        _parameters: &Self::Parameters,
        delta: &Self::Delta,
    ) -> Result<(), String> {
        let max_recent_messages = parent_state.configuration.configuration.max_recent_messages;
        let max_message_size = parent_state.configuration.configuration.max_message_size;
        self.messages.extend(delta.iter().cloned());

        // Ensure there are no messages over the size limit
        self.messages
            .retain(|m| m.message.content.len() <= max_message_size);

        // Sort messages by time
        self.messages
            .sort_by(|a, b| a.message.time.cmp(&b.message.time));

        // Ensure all messages are signed by a valid member, remove if not
        let members_by_id = parent_state.members.members_by_member_id();
        self.messages
            .retain(|m| members_by_id.contains_key(&m.message.author));

        // Remove oldest messages if there are too many
        while self.messages.len() > max_recent_messages {
            self.messages.remove(0);
        }

        Ok(())
    }
}

impl Default for MessagesV1 {
    fn default() -> Self {
        Self {
            messages: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct MessageV1 {
    pub owner_member_id: MemberId,
    pub author: MemberId,
    pub time: SystemTime,
    pub content: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthorizedMessageV1 {
    pub message: MessageV1,
    pub signature: Signature,
}

impl fmt::Debug for AuthorizedMessageV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthorizedMessage")
            .field("message", &self.message)
            .field(
                "signature",
                &format_args!("{}", truncated_base64(self.signature.to_bytes())),
            )
            .finish()
    }
}

#[derive(Eq, PartialEq, Hash, Serialize, Deserialize, Clone, Debug, Ord, PartialOrd)]
pub struct MessageId(pub FastHash);

impl AuthorizedMessageV1 {
    pub fn new(message: MessageV1, signing_key: &SigningKey) -> Self {
        Self {
            message: message.clone(),
            signature: sign_struct(&message, signing_key),
        }
    }

    pub fn validate(
        &self,
        verifying_key: &VerifyingKey,
    ) -> Result<(), ed25519_dalek::SignatureError> {
        verify_struct(&self.message, &self.signature, &verifying_key)
    }

    pub fn id(&self) -> MessageId {
        MessageId(fast_hash(&self.signature.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;
    use std::collections::HashMap;

    fn create_test_message(owner_id: MemberId, author_id: MemberId) -> MessageV1 {
        MessageV1 {
            owner_member_id: owner_id,
            author: author_id,
            time: SystemTime::now(),
            content: "Test message".to_string(),
        }
    }

    #[test]
    fn test_authorized_message_new_and_validate() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let owner_id = MemberId(FastHash(0));
        let author_id = MemberId(FastHash(1));

        let message = create_test_message(owner_id, author_id);
        let authorized_message = AuthorizedMessageV1::new(message.clone(), &signing_key);

        assert_eq!(authorized_message.message, message);
        assert!(authorized_message.validate(&verifying_key).is_ok());

        // Test with wrong key
        let wrong_key = SigningKey::generate(&mut OsRng).verifying_key();
        assert!(authorized_message.validate(&wrong_key).is_err());
    }

    #[test]
    fn test_message_id() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let owner_id = MemberId(FastHash(0));
        let author_id = MemberId(FastHash(1));

        let message = create_test_message(owner_id, author_id);
        let authorized_message = AuthorizedMessageV1::new(message, &signing_key);

        let id1 = authorized_message.id();
        let id2 = authorized_message.id();

        assert_eq!(id1, id2);
    }

    #[test]
    fn test_messages_verify() {
        // Generate a new signing key and its corresponding verifying key for the owner
        let owner_signing_key = SigningKey::generate(&mut OsRng);
        let owner_verifying_key = owner_signing_key.verifying_key();
        let owner_id = MemberId::new(&owner_verifying_key);

        // Generate a signing key for the author
        let author_signing_key = SigningKey::generate(&mut OsRng);
        let author_verifying_key = author_signing_key.verifying_key();
        let author_id = MemberId::new(&author_verifying_key);

        // Create a test message and authorize it with the author's signing key
        let message = create_test_message(owner_id, author_id);
        let authorized_message = AuthorizedMessageV1::new(message, &author_signing_key);

        // Create a Messages struct with the authorized message
        let messages = MessagesV1 {
            messages: vec![authorized_message],
        };

        // Set up a parent state (ChatRoomState) with the author as a member
        let mut parent_state = ChatRoomStateV1::default();
        let author_member = crate::state::member::Member {
            owner_member_id: owner_id,
            invited_by: owner_id,
            member_vk: author_verifying_key,
            nickname: "Author User".to_string(),
        };
        let authorized_author =
            crate::state::member::AuthorizedMember::new(author_member, &owner_signing_key);
        parent_state.members.members = vec![authorized_author];

        // Set up parameters for verification
        let parameters = ChatRoomParametersV1 {
            owner: owner_verifying_key,
        };

        // Verify that a valid message passes verification
        assert!(
            messages.verify(&parent_state, &parameters).is_ok(),
            "Valid messages should pass verification: {:?}",
            messages.verify(&parent_state, &parameters)
        );

        // Test with invalid signature
        let mut invalid_messages = messages.clone();
        invalid_messages.messages[0].signature = Signature::from_bytes(&[0; 64]); // Replace with an invalid signature
        assert!(
            invalid_messages.verify(&parent_state, &parameters).is_err(),
            "Messages with invalid signature should fail verification"
        );
    }

    #[test]
    fn test_messages_summarize() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let owner_id = MemberId(FastHash(0));
        let author_id = MemberId(FastHash(1));

        let message1 = create_test_message(owner_id, author_id);
        let message2 = create_test_message(owner_id, author_id);

        let authorized_message1 = AuthorizedMessageV1::new(message1, &signing_key);
        let authorized_message2 = AuthorizedMessageV1::new(message2, &signing_key);

        let messages = MessagesV1 {
            messages: vec![authorized_message1.clone(), authorized_message2.clone()],
        };

        let parent_state = ChatRoomStateV1::default();
        let parameters = ChatRoomParametersV1 {
            owner: signing_key.verifying_key(),
        };

        let summary = messages.summarize(&parent_state, &parameters);
        assert_eq!(summary.len(), 2);
        assert_eq!(summary[0], authorized_message1.id());
        assert_eq!(summary[1], authorized_message2.id());
    }

    #[test]
    fn test_messages_delta() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let owner_id = MemberId(FastHash(0));
        let author_id = MemberId(FastHash(1));

        let message1 = create_test_message(owner_id, author_id);
        let message2 = create_test_message(owner_id, author_id);
        let message3 = create_test_message(owner_id, author_id);

        let authorized_message1 = AuthorizedMessageV1::new(message1, &signing_key);
        let authorized_message2 = AuthorizedMessageV1::new(message2, &signing_key);
        let authorized_message3 = AuthorizedMessageV1::new(message3, &signing_key);

        let messages = MessagesV1 {
            messages: vec![
                authorized_message1.clone(),
                authorized_message2.clone(),
                authorized_message3.clone(),
            ],
        };

        let parent_state = ChatRoomStateV1::default();
        let parameters = ChatRoomParametersV1 {
            owner: signing_key.verifying_key(),
        };

        let old_summary = vec![authorized_message1.id(), authorized_message2.id()];
        let delta = messages.delta(&parent_state, &parameters, &old_summary).unwrap();

        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0], authorized_message3);
    }

    #[test]
    fn test_messages_apply_delta() {
        let owner_signing_key = SigningKey::generate(&mut OsRng);
        let owner_verifying_key = owner_signing_key.verifying_key();
        let owner_id = MemberId::new(&owner_verifying_key);

        let author_signing_key = SigningKey::generate(&mut OsRng);
        let author_verifying_key = author_signing_key.verifying_key();
        let author_id = MemberId::new(&author_verifying_key);

        let message1 = create_test_message(owner_id, author_id);
        let message2 = create_test_message(owner_id, author_id);
        let message3 = create_test_message(owner_id, author_id);

        let authorized_message1 = AuthorizedMessageV1::new(message1, &author_signing_key);
        let authorized_message2 = AuthorizedMessageV1::new(message2, &author_signing_key);
        let authorized_message3 = AuthorizedMessageV1::new(message3, &author_signing_key);

        let mut messages = MessagesV1 {
            messages: vec![authorized_message1.clone(), authorized_message2.clone()],
        };

        let mut parent_state = ChatRoomStateV1::default();
        parent_state.configuration.configuration.max_recent_messages = 3;
        parent_state.configuration.configuration.max_message_size = 100;
        parent_state.members.members = vec![crate::state::member::AuthorizedMember {
            member: crate::state::member::Member {
                owner_member_id: owner_id,
                invited_by: owner_id,
                member_vk: author_verifying_key,
                nickname: "Test User".to_string(),
            },
            signature: owner_signing_key.try_sign(&[0; 32]).unwrap(), // Sign with owner's key (simplified for test)
        }];

        let parameters = ChatRoomParametersV1 {
            owner: owner_verifying_key,
        };

        let delta = vec![authorized_message3.clone()];
        assert!(messages
            .apply_delta(&parent_state, &parameters, &delta)
            .is_ok());

        assert_eq!(
            messages.messages.len(),
            3,
            "Expected 3 messages after applying delta"
        );
        assert_eq!(
            messages.messages[0], authorized_message1,
            "First message should be authorized_message1"
        );
        assert_eq!(
            messages.messages[1], authorized_message2,
            "Second message should be authorized_message2"
        );
        assert_eq!(
            messages.messages[2], authorized_message3,
            "Third message should be authorized_message3"
        );

        // Test max_recent_messages limit
        let message4 = create_test_message(owner_id, author_id);
        let authorized_message4 = AuthorizedMessageV1::new(message4, &author_signing_key);
        let delta = vec![authorized_message4.clone()];
        assert!(messages
            .apply_delta(&parent_state, &parameters, &delta)
            .is_ok());

        assert_eq!(
            messages.messages.len(),
            3,
            "Expected 3 messages after applying delta with max_recent_messages limit"
        );
        assert_eq!(
            messages.messages[0], authorized_message2,
            "First message should be authorized_message2 after applying max_recent_messages limit"
        );
        assert_eq!(
            messages.messages[1], authorized_message3,
            "Second message should be authorized_message3 after applying max_recent_messages limit"
        );
        assert_eq!(
            messages.messages[2], authorized_message4,
            "Third message should be authorized_message4 after applying max_recent_messages limit"
        );
    }
}
