mod freenet_api;

use super::{conversation::Conversation, members::MemberList, room_list::RoomList};
use crate::components::app::freenet_api::FreenetApiSynchronizer;
use crate::components::members::member_info_modal::MemberInfoModal;
use crate::components::room_list::edit_room_modal::EditRoomModal;
use crate::components::room_list::ReceiveInvitationModal;
use crate::room_data::{CurrentRoom, Rooms};
use crate::components::members::Invitation;
use dioxus::prelude::*;
use document::Stylesheet;
use ed25519_dalek::VerifyingKey;
use river_common::room_state::member::MemberId;
use web_sys::window;

pub fn App() -> Element {
    use_context_provider(|| Signal::new(initial_rooms()));
    use_context_provider(|| Signal::new(CurrentRoom { owner_key: None }));
    use_context_provider(|| Signal::new(MemberInfoModalSignal { member: None }));
    use_context_provider(|| Signal::new(EditRoomModalSignal { room: None }));
    use_context_provider(|| Signal::new(CreateRoomModalSignal { show: false }));
    
    let receive_invitation_active = use_signal(|| false);
    let receive_invitation = use_signal(|| None::<Invitation>);

    // Check URL for invitation parameter
    if let Some(window) = window() {
        if let Ok(search) = window.location().search() {
            if let Some(params) = web_sys::UrlSearchParams::new_with_str(&search).ok() {
                if let Some(invitation_code) = params.get("invitation") {
                    if let Ok(invitation) = Invitation::from_encoded_string(&invitation_code) {
                        receive_invitation.set(Some(invitation));
                        receive_invitation_active.set(true);
                    }
                }
            }
        }
    }

    #[cfg(not(feature = "no-sync"))]
    {
        FreenetApiSynchronizer::start();
    }

    rsx! {
        Stylesheet { href: asset!("./assets/bulma.min.css") }
        Stylesheet { href: asset!("./assets/main.css") }
        Stylesheet { href: asset!("./assets/fontawesome/css/all.min.css") }
        div { class: "chat-container",
            RoomList {}
            Conversation {}
            MemberList {}
        }
        EditRoomModal {}
        MemberInfoModal {}
        ReceiveInvitationModal {
            is_active: receive_invitation_active,
            invitation: receive_invitation
        }
    }
}

#[cfg(not(feature = "example-data"))]
fn initial_rooms() -> Rooms {
    Rooms {
        map: std::collections::HashMap::new(),
    }
}

#[cfg(feature = "example-data")]
fn initial_rooms() -> Rooms {
    crate::example_data::create_example_rooms()
}

pub struct EditRoomModalSignal {
    pub room: Option<VerifyingKey>,
}

pub struct CreateRoomModalSignal {
    pub show: bool,
}

pub struct MemberInfoModalSignal {
    pub member: Option<MemberId>,
}
