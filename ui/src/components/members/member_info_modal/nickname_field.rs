use dioxus::prelude::*;
use dioxus::events::Key;
use std::rc::Rc;
use dioxus_logger::tracing::{error, warn};
use common::room_state::{ChatRoomParametersV1, ChatRoomStateV1Delta};
use common::room_state::member::MemberId;
use common::room_state::member_info::{AuthorizedMemberInfo, MemberInfo};
use freenet_scaffold::ComposableState;
use crate::room_data::{CurrentRoom, Rooms};

#[component]
pub fn NicknameField(member_info: AuthorizedMemberInfo) -> Element {
    // Retrieve contexts
    let rooms = use_context::<Signal<Rooms>>();
    let current_room = use_context::<Signal<CurrentRoom>>();

    // Compute values
    let self_signing_key = {
        let rooms = rooms.read();
        let current_room = current_room.read();
        current_room
            .owner_key
            .as_ref()
            .and_then(|key| rooms.map.get(key))
            .map(|room_data| room_data.self_sk.clone())
    };

    let self_member_id = self_signing_key
        .as_ref()
        .map(|sk| MemberId::from(&sk.verifying_key()));

    let member_id = member_info.member_info.member_id;
    let is_self = self_member_id
        .as_ref()
        .map(|smi| smi == &member_id)
        .unwrap_or(false);

    let mut temp_nickname = use_signal(|| member_info.member_info.preferred_nickname.clone());
    let mut input_element = use_signal(|| None as Option<Rc<MountedData>>);

    let save_changes = {
        let mut rooms = rooms.clone();
        let current_room = current_room.clone();
        let self_signing_key = self_signing_key.clone();
        let member_info = member_info.clone();

        move |new_value: String| {
            if new_value.is_empty() {
                warn!("Nickname cannot be empty");
                return;
            }

            let delta = if let Some(signing_key) = self_signing_key.clone() {
                let new_member_info = MemberInfo {
                    member_id: member_info.member_info.member_id.clone(),
                    version: member_info.member_info.version + 1,
                    preferred_nickname: new_value,
                };
                let new_authorized_member_info =
                    AuthorizedMemberInfo::new_with_member_key(new_member_info, &signing_key);
                Some(ChatRoomStateV1Delta {
                    recent_messages: None,
                    configuration: None,
                    bans: None,
                    members: None,
                    member_info: Some(vec![new_authorized_member_info]),
                    upgrade: None,
                })
            } else {
                warn!("No signing key available");
                None
            };

            if let Some(delta) = delta {
                let mut rooms = rooms.write();
                if let Some(owner_key) = current_room.read().owner_key.clone() {
                    if let Some(room_data) = rooms.map.get_mut(&owner_key) {
                        if let Err(e) = room_data.room_state.apply_delta(
                            &room_data.room_state.clone(),
                            &ChatRoomParametersV1 { owner: owner_key },
                            &Some(delta),
                        ) {
                            error!("Failed to apply delta: {:?}", e);
                        }
                    } else {
                        warn!("Room state not found for current room");
                    }
                }
            }
        }
    };

    let on_input = move |evt: Event<FormData>| {
        temp_nickname.set(evt.value().clone());
    };

    let on_blur = {
        let mut save_changes = save_changes.clone();
        let temp_nickname = temp_nickname.clone();
        move |_| {
            let new_value = temp_nickname();
            save_changes(new_value);
        }
    };

    let on_keydown = {
        let mut save_changes = save_changes.clone();
        let temp_nickname = temp_nickname.clone();
        move |evt: Event<KeyboardData>| {
            if evt.key() == Key::Enter {
                let new_value = temp_nickname();
                save_changes(new_value);
                
                // Blur the input element
                if let Some(element) = input_element() {
                    wasm_bindgen_futures::spawn_local(async move {
                        let _ = element.set_focus(false).await;
                    });
                }
            }
        }
    };

    rsx! {
        div {
            class: "field",
            label { class: "label", "Nickname" }
            div {
                class: if is_self { "control has-icons-right" } else { "control" },
                input {
                    class: "input",
                    value: "{temp_nickname}",
                    readonly: !is_self,
                    oninput: on_input,
                    onblur: on_blur,
                    onkeydown: on_keydown,
                    onmounted: move |cx| input_element.set(Some(cx.data())),
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
