use dioxus::prelude::*;
use dioxus_logger::tracing::{error, warn};
use common::room_state::{ChatRoomParametersV1, ChatRoomStateV1Delta};
use common::room_state::member::{AuthorizedMember, MemberId};
use common::room_state::member_info::{AuthorizedMemberInfo, MemberInfo};
use freenet_scaffold::ComposableState;
use crate::room_data::{CurrentRoom, Rooms};
use crate::util::use_current_room_data;

#[component]
pub fn NicknameField(
    member: AuthorizedMember,
    member_info: AuthorizedMemberInfo,
) -> Element {
    // Retrieve contexts
    let rooms = use_context::<Signal<Rooms>>();
    let current_room = use_context::<Signal<CurrentRoom>>();
    let current_room_data = use_current_room_data(rooms.clone(), current_room.clone());

    // Compute values
    let self_signing_key = current_room_data
        .read()
        .as_ref()
        .map(|room_data| room_data.self_sk.clone());

    let self_member_id = self_signing_key
        .as_ref()
        .map(|sk| MemberId::from(&sk.verifying_key()));

    let member_id = member.member.id();
    let is_self = self_member_id
        .as_ref()
        .map(|smi| smi == &member_id)
        .unwrap_or(false);

    // Create nickname signal initialized with the preferred nickname
    let nickname = use_signal(|| member_info.member_info.preferred_nickname.clone());

    // Effect to update nickname when member_info changes
    {
        let mut nickname = nickname.clone();
        let member_info_version = member_info.member_info.version;
        let preferred_nickname = member_info.member_info.preferred_nickname.clone();

        use_effect(move || {
            // Only update if the nickname is different
            if nickname() != preferred_nickname {
                nickname.set(preferred_nickname.clone());
            }
        });
    }

    // Event handler for updating nickname
    let update_nickname = {
        let mut nickname = nickname.clone();
        let mut rooms = rooms.clone();
        let current_room = current_room.clone();
        let self_signing_key = self_signing_key.clone();
        let member_info = member_info.clone();

        move |evt: Event<FormData>| {
            let new_nickname = evt.value().to_string();
            if !new_nickname.is_empty() {
                nickname.set(new_nickname.clone());

                let self_member_id = member_info.member_info.member_id.clone();
                let new_member_info = MemberInfo {
                    member_id: self_member_id,
                    version: member_info.member_info.version + 1,
                    preferred_nickname: new_nickname,
                };

                if let Some(signing_key) = self_signing_key.clone() {
                    let new_authorized_member_info =
                        AuthorizedMemberInfo::new_with_member_key(new_member_info, &signing_key);
                    let delta = ChatRoomStateV1Delta {
                        recent_messages: None,
                        configuration: None,
                        bans: None,
                        members: None,
                        member_info: Some(vec![new_authorized_member_info]),
                        upgrade: None,
                    };

                    let mut rooms_write_guard = rooms.write();
                    let owner_key = current_room.read().owner_key.clone().expect("No owner key");

                    if let Some(room_data) = rooms_write_guard.map.get_mut(&owner_key) {
                        if let Err(e) = room_data.room_state.apply_delta(
                            &room_data.room_state.clone(),
                            &ChatRoomParametersV1 { owner: owner_key },
                            &delta,
                        ) {
                            error!("Failed to apply delta: {:?}", e);
                        }
                    } else {
                        warn!("Room state not found for current room");
                    }
                } else {
                    warn!("No signing key available");
                }
            } else {
                warn!("Nickname is empty");
            }
        }
    };

    // Render the component
    rsx! {
        div { class: "field",
            label { class: "label", "Nickname" }
            div { class: if is_self { "control has-icons-right" } else { "control" },
                input {
                    class: "input",
                    value: "{nickname}",
                    readonly: !is_self,
                    onchange: update_nickname,
                }
                if is_self {
                    span {
                        class: "icon is-right",
                        i {
                            class: "fa-solid fa-pencil"
                        }
                    }
                }
            }
        }
    }
}
