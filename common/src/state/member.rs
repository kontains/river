use std::collections::{HashMap, HashSet};
use crate::util::{sign_struct, truncated_base64, verify_struct};
use ed25519_dalek::{Signature, VerifyingKey, SigningKey, Signer, Verifier};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::{Hash, Hasher};
use freenet_scaffold::ComposableState;
use freenet_scaffold::util::{fast_hash, FastHash};
use crate::ChatRoomState;
use crate::state::ChatRoomParameters;

#[derive(Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct Members {
    pub members: Vec<AuthorizedMember>,
}

impl Default for Members {
    fn default() -> Self {
        Members { members: Vec::new() }
    }
}

impl ComposableState for Members {
    type ParentState = ChatRoomState;
    type Summary = HashSet<MemberId>;
    type Delta = MembersDelta;
    type Parameters = ChatRoomParameters;

    fn verify(&self, _parent_state: &Self::ParentState, parameters: &Self::Parameters) -> Result<(), String> {
        if self.members.is_empty() {
            return Ok(());
        }
        let owner_id = parameters.owner_id();
        for member in &self.members {
            if member.member.id() == owner_id {
                return Err("Owner should not be included in the members list".to_string());
            }
            // Navigate up the invite chain and ensure that it ends with the owner, if it doesn't then
            // verification fails. 
            let mut current_member = member;
            loop {
                if current_member.member.invited_by == owner_id {
                    // Member was directly invited by the owner, so we need to verify their signature against the owner's key
                    current_member.verify_signature(&parameters.owner)
                        .map_err(|e| format!("Invalid signature for member {:?} invited by owner: {}", current_member.member.id(), e))?;
                    break;
                } else {
                    let inviter = self.members.iter().find(|m| m.member.id() == current_member.member.invited_by)
                        .ok_or_else(|| format!("Inviter {:?} not found for member {:?}", current_member.member.invited_by, current_member.member.id()))?;
                    
                    current_member.verify_signature(&inviter.member.member_vk)
                        .map_err(|e| format!("Invalid signature for member {:?}: {}", current_member.member.id(), e))?;
                    
                    current_member = inviter;
                }
            }
        }
        Ok(())
    }

    fn summarize(&self, _parent_state: &Self::ParentState, _parameters: &Self::Parameters) -> Self::Summary {
        self.members.iter().map(|m| m.member.id()).collect()
    }

    fn delta(&self, _parent_state: &Self::ParentState, _parameters: &Self::Parameters, old_state_summary: &Self::Summary) -> Self::Delta {
        let added = self.members.iter().filter(|m| !old_state_summary.contains(&m.member.id())).cloned().collect::<Vec<_>>();
        let removed = old_state_summary.iter().filter(|m| !self.members.iter().any(|am| &am.member.id() == *m)).cloned().collect::<Vec<_>>();
        MembersDelta { added, removed }
    }

    fn apply_delta(&self, parent_state: &Self::ParentState, _parameters: &Self::Parameters, delta: &Self::Delta) -> Self {
        let mut members = self.members.clone();
        members.retain(|m| !delta.removed.contains(&m.member.id()));
        members.extend(delta.added.iter().cloned());
        let max_members = parent_state.configuration.configuration.max_members;
        while members.len() > max_members {
            todo!()
        }
        Members { members }
    }
}

impl Members {
    pub fn members_by_member_id(&self) -> HashMap<MemberId, &Member> {
        self.members.iter().map(|m| (m.member.id(), &m.member)).collect()
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Clone)]
pub struct MembersDelta {
    added: Vec<AuthorizedMember>,
    removed: Vec<MemberId>,
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Clone, Debug)]
pub struct AuthorizedMember {
    pub member: Member,
    pub signature: Signature,
}

impl AuthorizedMember {
    pub fn new(member: Member, inviter_signing_key: &SigningKey) -> Self {
        Self {
            member: member.clone(),
            signature: sign_struct(&member, inviter_signing_key),
        }
    }
    
    pub fn verify_signature(&self, inviter_vk: &VerifyingKey) -> Result<(), String> {
        verify_struct(&self.member, &self.signature, inviter_vk).map_err(|e| format!("Invalid signature: {}", e))
    }
}

impl Hash for AuthorizedMember {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.member.hash(state);
    }
}

#[derive(Serialize, Deserialize, Eq, PartialEq, Hash, Clone)]
pub struct Member {
    pub owner_member_id: MemberId,
    pub invited_by: MemberId,
    pub member_vk: VerifyingKey,
    pub nickname: String,
}

impl fmt::Debug for Member {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Member")
            .field("public_key", &format_args!("{}", truncated_base64(self.member_vk.as_bytes())))
            .field("nickname", &self.nickname)
            .finish()
    }
}

#[derive(Eq, PartialEq, Hash, Serialize, Deserialize, Clone, Debug, Ord, PartialOrd, Copy)]
pub struct MemberId(pub FastHash);

impl MemberId {
    pub fn new(member_vk: &VerifyingKey) -> Self {
        MemberId(fast_hash(&member_vk.to_bytes()))
    }
}

impl Member {
    pub fn id(&self) -> MemberId {
        MemberId::new(&self.member_vk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use ed25519_dalek::SigningKey;

    fn create_test_member(owner_id: MemberId, invited_by: MemberId) -> Member {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        Member {
            owner_member_id: owner_id,
            invited_by,
            member_vk: verifying_key,
            nickname: "Test User".to_string(),
        }
    }

    #[test]
    fn test_members_verify() {
        let owner_signing_key = SigningKey::generate(&mut OsRng);
        let owner_verifying_key = VerifyingKey::from(&owner_signing_key);
        let owner_id = MemberId::new(&owner_verifying_key);

        let member1 = create_test_member(owner_id, owner_id);
        let member2 = create_test_member(owner_id, member1.id());

        let authorized_member1 = AuthorizedMember::new(member1.clone(), &owner_signing_key);
        let authorized_member2 = AuthorizedMember::new(member2.clone(), &owner_signing_key);

        let members = Members {
            members: vec![authorized_member1.clone(), authorized_member2.clone()],
        };

        println!("Member1 ID: {:?}", member1.id());
        println!("Member2 ID: {:?}", member2.id());
        println!("Owner ID: {:?}", owner_id);

        let parent_state = ChatRoomState::default();
        let parameters = ChatRoomParameters {
            owner: owner_verifying_key,
        };

        let result = members.verify(&parent_state, &parameters);
        println!("Verification result: {:?}", result);
        assert!(result.is_ok(), "Verification failed: {:?}", result);

        // Test that including the owner in the members list fails verification
        let owner_member = create_test_member(owner_id, owner_id);
        let authorized_owner = AuthorizedMember::new(owner_member, &owner_signing_key);
        let members_with_owner = Members {
            members: vec![authorized_owner, authorized_member1, authorized_member2],
        };
        let result_with_owner = members_with_owner.verify(&parent_state, &parameters);
        println!("Verification result with owner: {:?}", result_with_owner);
        assert!(result_with_owner.is_err(), "Verification should fail when owner is included: {:?}", result_with_owner);
    }

    #[test]
    fn test_members_summarize() {
        let owner_id = MemberId(FastHash(0));
        let member1 = create_test_member(owner_id, owner_id);
        let member2 = create_test_member(owner_id, member1.id());

        let signing_key = SigningKey::generate(&mut OsRng);

        let authorized_member1 = AuthorizedMember::new(member1.clone(), &signing_key);
        let authorized_member2 = AuthorizedMember::new(member2.clone(), &signing_key);

        let members = Members {
            members: vec![authorized_member1, authorized_member2],
        };

        let parent_state = ChatRoomState::default();
        let parameters = ChatRoomParameters {
            owner: VerifyingKey::from(&SigningKey::generate(&mut OsRng)),
        };

        let summary = members.summarize(&parent_state, &parameters);
        assert_eq!(summary.len(), 2);
        assert!(summary.contains(&member1.id()));
        assert!(summary.contains(&member2.id()));
    }

    #[test]
    fn test_members_delta() {
        let owner_id = MemberId(FastHash(0));
        let member1 = create_test_member(owner_id, owner_id);
        let member2 = create_test_member(owner_id, member1.id());
        let member3 = create_test_member(owner_id, member1.id());

        let signing_key = SigningKey::generate(&mut OsRng);

        let authorized_member1 = AuthorizedMember::new(member1.clone(), &signing_key);
        let authorized_member2 = AuthorizedMember::new(member2.clone(), &signing_key);
        let authorized_member3 = AuthorizedMember::new(member3.clone(), &signing_key);

        let old_members = Members {
            members: vec![authorized_member1.clone(), authorized_member2.clone()],
        };

        let new_members = Members {
            members: vec![authorized_member1.clone(), authorized_member3.clone()],
        };

        let parent_state = ChatRoomState::default();
        let parameters = ChatRoomParameters {
            owner: VerifyingKey::from(&SigningKey::generate(&mut OsRng)),
        };

        let old_summary = old_members.summarize(&parent_state, &parameters);
        let delta = new_members.delta(&parent_state, &parameters, &old_summary);

        assert_eq!(delta.added.len(), 1);
        assert_eq!(delta.added[0].member.id(), member3.id());
        assert_eq!(delta.removed.len(), 1);
        assert_eq!(delta.removed[0], member2.id());
    }

    #[test]
    fn test_members_apply_delta() {
        let owner_id = MemberId(FastHash(0));
        let member1 = create_test_member(owner_id, owner_id);
        let member2 = create_test_member(owner_id, member1.id());
        let member3 = create_test_member(owner_id, member1.id());

        let signing_key = SigningKey::generate(&mut OsRng);

        let authorized_member1 = AuthorizedMember::new(member1.clone(), &signing_key);
        let authorized_member2 = AuthorizedMember::new(member2.clone(), &signing_key);
        let authorized_member3 = AuthorizedMember::new(member3.clone(), &signing_key);

        let old_members = Members {
            members: vec![authorized_member1.clone(), authorized_member2.clone()],
        };

        let delta = MembersDelta {
            added: vec![authorized_member3.clone()],
            removed: vec![member2.id()],
        };

        let mut parent_state = ChatRoomState::default();
        parent_state.configuration.configuration.max_members = 3;

        let parameters = ChatRoomParameters {
            owner: VerifyingKey::from(&SigningKey::generate(&mut OsRng)),
        };

        let new_members = old_members.apply_delta(&parent_state, &parameters, &delta);

        assert_eq!(new_members.members.len(), 2);
        assert!(new_members.members.iter().any(|m| m.member.id() == member1.id()));
        assert!(new_members.members.iter().any(|m| m.member.id() == member3.id()));
        assert!(!new_members.members.iter().any(|m| m.member.id() == member2.id()));
    }

    #[test]
    fn test_authorized_member_validate() {
        let owner_signing_key = SigningKey::generate(&mut OsRng);
        let owner_verifying_key = VerifyingKey::from(&owner_signing_key);
        let owner_id = MemberId::new(&owner_verifying_key);

        let member1 = create_test_member(owner_id, owner_id);
        let member2 = create_test_member(owner_id, member1.id());

        let authorized_member1 = AuthorizedMember::new(member1.clone(), &owner_signing_key);
        let member2_signing_key = SigningKey::generate(&mut OsRng);
        let authorized_member2 = AuthorizedMember::new(member2.clone(), &member2_signing_key);

        let members = vec![authorized_member1.clone(), authorized_member2.clone()];

     //   assert!(authorized_member2.verify(&members));

        // Test with invalid signature
        let invalid_member2 = AuthorizedMember {
            member: member2.clone(),
            signature: Signature::from_bytes(&[0; 64]),
        };
     //   assert!(!invalid_member2.verify(&members));
    }

    #[test]
    fn test_member_id() {
        let owner_id = MemberId(FastHash(0));
        let member = create_test_member(owner_id, owner_id);
        let member_id = member.id();

        assert_eq!(member_id, MemberId::new(&member.member_vk));
    }
}

