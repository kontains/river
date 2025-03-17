use crate::components::app::{CURRENT_ROOM, ROOMS};
use crate::components::members::Invitation;
use crate::room_data::{CurrentRoom, RoomData, Rooms};
use dioxus::prelude::*;
use ed25519_dalek::SigningKey;
use river_common::room_state::member::{AuthorizedMember, Member};
use std::rc::Rc;
use wasm_bindgen::JsCast;

const BASE_URL: &str =
    "http://127.0.0.1:50509/v1/contract/web/C8tm2U616vC2dBo8ffWoc8YL9yJGyKJ5C4Y2Nfm2YAn5/";

#[component]
pub fn InviteMemberModal(is_active: Signal<bool>) -> Element {
    // Add a signal to track when a new invitation is generated
    let regenerate_trigger = use_signal(|| 0);
    
    let current_room_data_signal: Memo<Option<RoomData>> = use_memo(move || {
        CURRENT_ROOM
            .read()
            .owner_key
            .as_ref()
            .and_then(|key| ROOMS.read().map.get(key).cloned())
    });

    let invitation_future = use_resource(move || {
        let _trigger = *regenerate_trigger.read(); // Use underscore to indicate intentional unused variable
        // Using trigger value to force re-execution when regenerate_trigger changes
        async move {
            if !*is_active.read() {
                return Err("Modal closed".to_string());
            }
            let room_data = current_room_data_signal();
            if let Some(room_data) = room_data {
                // Generate new signing key for invitee
                let invitee_signing_key = SigningKey::generate(&mut rand::thread_rng());
                let invitee_verifying_key = invitee_signing_key.verifying_key();

                // Create member struct
                let member = Member {
                    owner_member_id: room_data.owner_vk.into(),
                    invited_by: room_data.self_sk.verifying_key().into(),
                    member_vk: invitee_verifying_key,
                };

                // Create authorized member with signature
                let authorized_member = AuthorizedMember::new(member, &room_data.self_sk);

                // Create invitation
                let invitation = Invitation {
                    room: room_data.owner_vk,
                    invitee_signing_key,
                    invitee: authorized_member,
                };

                Ok::<Invitation, String>(invitation)
            } else {
                Err("No room selected".to_string())
            }
        }
    });

    rsx! {
        div {
            class: if *is_active.read() { "modal is-active" } else { "modal" },
            div {
                class: "modal-background",
                onclick: move |_| {
                    is_active.set(false);
                }
            }
            div { class: "modal-content",
                div { class: "box",
                    match &*invitation_future.read_unchecked() {
                        Some(Ok(invitation)) => {
                            let room_name = current_room_data_signal()
                                .map(|r| r.room_state.configuration.configuration.name.clone())
                                .unwrap_or_else(|| "this chat room".to_string());

                            let invite_code = invitation.to_encoded_string();
                            let invite_url = format!("{}?invitation={}", BASE_URL, invite_code);

                            let default_msg = format!(
                                "You've been invited to join the chat room \"{}\"!\n\n\
                                To join:\n\
                                1. Install Freenet from https://freenet.org\n\
                                2. Open this link:\n\
                                {}\n\n\
                                Note: Keep this invitation private - anyone with this link can join as you.",
                                room_name, invite_url
                            );

                            rsx! {
                                h3 { class: "title is-4", "Invitation Generated" }

                                div { class: "message is-info",
                                    div { class: "message-body",
                                        "Important: Share this invitation only with the intended person. Anyone with this link can join the room and impersonate them."                                    }
                                }

                                InvitationContent {
                                    invitation_text: default_msg,
                                    is_active: is_active,
                                    regenerate_trigger: regenerate_trigger
                                }
                            }
                        }
                        Some(Err(err)) => {
                            rsx! {
                                h3 { class: "title is-4", "Error" }
                                p { class: "has-text-danger", "{err}" }
                                button {
                                    class: "button",
                                    onclick: move |_| {
                                        is_active.set(false);
                                        is_active.set(true);
                                    },
                                    "Try Again"
                                }
                            }
                        },
                        None => {
                            rsx! {
                                h3 { class: "title is-4", "Generating Invitation..." }
                                progress { class: "progress is-small is-primary" }
                            }
                        }
                    }
                }
            }
            button {
                class: "modal-close is-large",
                onclick: move |_| is_active.set(false)
            }
        }
    }
}
#[component]
fn InvitationContent(
    invitation_text: String,
    is_active: Signal<bool>,
    regenerate_trigger: Signal<i32>,
) -> Element {
    let mut copy_text = use_signal(|| "Copy Invitation".to_string());
    // Don't create a signal from the initial invitation_text
    // We'll use the current value from the parent component directly

    // Clone the invitation text for use in the closure
    let invitation_text_for_clipboard = invitation_text.clone();
    
    let copy_to_clipboard = move |_| {
        if let Some(window) = web_sys::window() {
            if let Ok(navigator) = window.navigator().dyn_into::<web_sys::Navigator>() {
                let clipboard = navigator.clipboard();
                let _ = clipboard.write_text(&invitation_text_for_clipboard);
                copy_text.set("Copied!".to_string());
            }
        }
    };

    rsx! {
        div { class: "field",
            label { class: "label", "Invitation message:" }
            div {
                class: "box",
                style: "white-space: pre-wrap; font-family: monospace;",
                "{invitation_text}"
            }
        }

        div { class: "field is-grouped",
            div { class: "control",
                button {
                    class: "button is-primary",
                    onclick: copy_to_clipboard,
                    span { class: "icon", i { class: "fas fa-copy" } }
                    span { "{copy_text}" }
                }
            }
            div { class: "control",
                button {
                    class: "button",
                    onclick: move |_| {
                        // Increment the regenerate trigger to force a new invitation
                        let current_value = *regenerate_trigger.read();
                        regenerate_trigger.set(current_value + 1);
                    },
                    "Generate New"
                }
            }
            div { class: "control",
                button {
                    class: "button",
                    onclick: move |_| is_active.set(false),
                    "Close"
                }
            }
        }
    }
}
