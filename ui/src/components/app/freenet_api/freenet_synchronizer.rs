use super::constants::*;
use crate::invites::PendingInvites;
use crate::room_data::{Rooms, RoomData, RoomSyncStatus};
use crate::util::{from_cbor_slice, owner_vk_to_contract_key, sleep, to_cbor_vec};
use dioxus::prelude::*;
use dioxus::logger::tracing::{info, error, debug, warn};
use ed25519_dalek::VerifyingKey;
use std::collections::{HashMap};
use std::sync::Arc;
use std::time::Duration;
use wasm_bindgen_futures::spawn_local;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::{SinkExt, StreamExt, TryFutureExt};
use freenet_scaffold::ComposableState;
use freenet_stdlib::{
    client_api::{WebApi, HostResponse, ClientRequest, ContractRequest, ContractResponse},
    prelude::{ContractKey},
};
use freenet_stdlib::prelude::{ContractCode, ContractContainer, ContractInstanceId, ContractWasmAPIVersion, Parameters, RelatedContracts, UpdateData, WrappedContract, WrappedState};
use river_common::room_state::{ChatRoomParametersV1, ChatRoomStateV1, ChatRoomStateV1Delta};
use crate::constants::ROOM_CONTRACT_WASM;

/// Message types for communicating with the synchronizer
enum SynchronizerMessage {
    ProcessRooms,
    Connect,
    ApiResponse(Result<HostResponse, String>),
}

/// Manages synchronization between local room state and Freenet network
pub struct FreenetSynchronizer {
    rooms: Signal<Rooms>,
    web_api: Option<WebApi>,
    // Used to show status in UI
    synchronizer_status: Signal<SynchronizerStatus>,
    contract_sync_info: HashMap<ContractInstanceId, ContractSyncInfo>,
    // Channel for sending messages to the synchronizer
    message_tx: UnboundedSender<SynchronizerMessage>,
    message_rx: Option<UnboundedReceiver<SynchronizerMessage>>,
}

struct ContractSyncInfo {
    owner_vk: VerifyingKey,
}

pub enum SynchronizerStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

impl FreenetSynchronizer {
    pub fn new(
        rooms: Signal<Rooms>,
        synchronizer_status: Signal<SynchronizerStatus>,
    ) -> Self {
        let (message_tx, message_rx) = unbounded();
        
        Self {
            rooms,
            synchronizer_status,
            web_api: None,
            contract_sync_info: HashMap::new(),
            message_tx,
            message_rx: Some(message_rx),
        }
    }

    pub async fn start(&mut self) {
        info!("Starting FreenetSynchronizer");
        
        // Take ownership of the receiver
        let mut message_rx = self.message_rx.take().expect("Message receiver already taken");
        let message_tx = self.message_tx.clone();
        
        // Clone signals for the effect
        let rooms = self.rooms.clone();
        let effect_tx = self.message_tx.clone();
        
        // Set up effect to monitor room changes
        use_effect(move || {
            info!("Rooms state changed, checking for sync needs");
            // Send a message to process rooms instead of calling directly
            spawn_local({
                let tx = effect_tx.clone();
                async move {
                    if let Err(e) = tx.unbounded_send(SynchronizerMessage::ProcessRooms) {
                        error!("Failed to send ProcessRooms message: {}", e);
                    }
                }
            });
        });

        // Start the message processing loop in a separate task
        let sync_status = self.synchronizer_status.clone();
        spawn_local(async move {
            // Start connection
            if let Err(e) = message_tx.unbounded_send(SynchronizerMessage::Connect) {
                error!("Failed to send Connect message: {}", e);
            }
            
            // Create a mutable synchronizer state that can be modified in the loop
            let mut sync_state = FreenetSynchronizerState {
                rooms,
                web_api: None,
                synchronizer_status: sync_status,
                contract_sync_info: HashMap::new(),
                message_tx: message_tx.clone(),
            };
            
            // Process messages
            while let Some(msg) = message_rx.next().await {
                match msg {
                    SynchronizerMessage::ProcessRooms => {
                        if let Err(e) = sync_state.process_rooms().await {
                            error!("Error processing rooms: {}", e);
                        }
                    },
                    SynchronizerMessage::Connect => {
                        sync_state.connect();
                    },
                    SynchronizerMessage::ApiResponse(response) => {
                        match response {
                            Ok(api_response) => {
                                if let Err(e) = sync_state.handle_api_response(api_response).await {
                                    error!("Error handling API response: {}", e);
                                }
                            },
                            Err(e) => error!("Error in API response: {}", e),
                        }
                    }
                }
            }
            
            warn!("Synchronizer message loop ended");
        });
    }

    // This is now just a message sender
    fn connect(&self) {
        if let Err(e) = self.message_tx.unbounded_send(SynchronizerMessage::Connect) {
            error!("Failed to send Connect message: {}", e);
        }
    }

}

// Separate state struct that can be modified in the message loop
struct FreenetSynchronizerState {
    rooms: Signal<Rooms>,
    web_api: Option<WebApi>,
    synchronizer_status: Signal<SynchronizerStatus>,
    contract_sync_info: HashMap<ContractInstanceId, ContractSyncInfo>,
    message_tx: UnboundedSender<SynchronizerMessage>,
}

impl FreenetSynchronizerState {
    async fn process_rooms(&mut self) -> Result<(), String> {
        // Get mutable access to rooms
        let mut rooms = self.rooms.write();
        
        // Iterate through rooms and check which ones need synchronization
        for (room_key, room_data) in rooms.map.iter_mut() {
            match room_data.sync_status {
                RoomSyncStatus::Disconnected => {
                    self.put_and_subscribe(room_key, room_data).await?;
                }

                _ => {} // No action needed for other states
            }
        }
        Ok(())
    }

    async fn put_and_subscribe(&mut self, owner_vk: &VerifyingKey, room_data: &mut RoomData) -> Result<(), String> {
        info!("Putting room state for: {:?}", owner_vk);

        let contract_key: ContractKey = owner_vk_to_contract_key(owner_vk);

        self.contract_sync_info.insert(*contract_key.id(), ContractSyncInfo {
            owner_vk: *owner_vk,
        });

        let contract_code = ContractCode::from(ROOM_CONTRACT_WASM);
        let parameters = ChatRoomParametersV1 { owner: *owner_vk };
        let params_bytes = to_cbor_vec(&parameters);
        let parameters = Parameters::from(params_bytes);

        let contract_container = ContractContainer::from(
            ContractWasmAPIVersion::V1(
                WrappedContract::new(
                    Arc::new(contract_code),
                    parameters,
                ),
            )
        );

        let state_bytes = to_cbor_vec(&room_data.room_state);
        let wrapped_state = WrappedState::new(state_bytes.clone());

        let put_request = ContractRequest::Put {
            contract: contract_container,
            state: wrapped_state,
            related_contracts: RelatedContracts::default(),
        };
        
        let client_request = ClientRequest::ContractOp(put_request);

        // Put the contract state
        if let Some(web_api) = &self.web_api {
            web_api.send(client_request)
                .map_err(|e| format!("Failed to put contract state: {}", e))?;
            
            room_data.sync_status = RoomSyncStatus::Putting;
        } else {
            return Err("Web API not initialized".to_string());
        }

        // Will subscribe when response comes back from PUT
        Ok(())
    }

    /// Initializes the connection to the Freenet node
    async fn initialize_connection(&mut self) -> Result<(), String> {
        info!("Connecting to Freenet node at: {}", WEBSOCKET_URL);
        *self.synchronizer_status.write() = SynchronizerStatus::Connecting;
        
        let websocket = web_sys::WebSocket::new(WEBSOCKET_URL).map_err(|e| {
            let error_msg = format!("Failed to create WebSocket: {:?}", e);
            error!("{}", error_msg);
            error_msg
        })?;

        // Create channel for API responses
        let (ready_tx, ready_rx) = futures::channel::oneshot::channel();
        let message_tx = self.message_tx.clone();
        
        let sync_status = self.synchronizer_status.clone();

        let web_api = WebApi::start(
            websocket.clone(),
            move |result| {
                let mapped_result = result.map_err(|e| e.to_string());
                spawn_local({
                    let tx = message_tx.clone();
                    async move {
                        if let Err(e) = tx.unbounded_send(SynchronizerMessage::ApiResponse(mapped_result)) {
                            error!("Failed to send API response: {}", e);
                        }
                    }
                });
            },
            move |error| {
                let error_msg = format!("WebSocket error: {}", error);
                error!("{}", error_msg);
                *sync_status.write() = SynchronizerStatus::Error(error_msg);
            },
            move || {
                info!("WebSocket connected successfully");
                *sync_status.write() = SynchronizerStatus::Connected;
                let _ = ready_tx.send(());
            },
        );

        let timeout = async {
            sleep(Duration::from_millis(CONNECTION_TIMEOUT_MS)).await;
            Err::<(), String>("WebSocket connection timed out".to_string())
        };

        match futures::future::select(Box::pin(ready_rx), Box::pin(timeout)).await {
            futures::future::Either::Left((Ok(_), _)) => {
                info!("WebSocket connection established successfully");
                self.web_api = Some(web_api);
                *self.synchronizer_status.write() = SynchronizerStatus::Connected;
                
                // Process rooms to sync them
                self.process_rooms().await?;
                
                Ok(())
            }
            _ => {
                let error_msg = "WebSocket connection failed or timed out".to_string();
                error!("{}", error_msg);
                *self.synchronizer_status.write() = SynchronizerStatus::Error(error_msg.clone());
                
                // Schedule reconnect
                let tx = self.message_tx.clone();
                spawn_local(async move {
                    sleep(Duration::from_millis(RECONNECT_INTERVAL_MS)).await;
                    if let Err(e) = tx.unbounded_send(SynchronizerMessage::Connect) {
                        error!("Failed to send reconnect message: {}", e);
                    }
                });
                
                Err(error_msg)
            }
        }
    }

    fn connect(&mut self) {
        spawn_local({
            let this = self;
            async move {
                if let Err(e) = this.initialize_connection().await {
                    error!("Failed to connect: {}", e);
                }
            }
        });
    }

    /// Handles individual API responses
    async fn handle_api_response(&mut self, response: HostResponse) -> Result<(), String> {
        match response {
            HostResponse::Ok => {
                info!("Received OK response from API");
            },
            HostResponse::ContractResponse(contract_response) => {
                match contract_response {
                    ContractResponse::GetResponse { key, contract, state } => {
                        warn!("GetResponse received for key {key} but not currently handled");
                    },
                    ContractResponse::PutResponse { key } => {
                        let contract_info = self.contract_sync_info.get(&key.id());
                        // Subscribe to the contract after PUT
                        if let Some(info) = contract_info {
                            let owner_vk = info.owner_vk;
                            let client_request = ClientRequest::ContractOp(ContractRequest::Subscribe {
                                key,
                                summary: None,
                            });
                            
                            if let Some(web_api) = &self.web_api {
                                web_api.send(client_request)
                                    .map_err(|e| format!("Failed to subscribe to contract: {}", e))?;
                                
                                // Update room status
                                let mut rooms = self.rooms.write();
                                if let Some(room_data) = rooms.map.get_mut(&owner_vk) {
                                    room_data.sync_status = RoomSyncStatus::Subscribing;
                                }
                            } else {
                                return Err("Web API not initialized".to_string());
                            }
                        } else {
                            warn!("Received PUT response for unknown contract: {:?}", key);
                        }
                    }
                    ContractResponse::UpdateNotification { key, update } => {
                        info!("Received update notification for key: {key}");
                        let contract_info = self.contract_sync_info.get(&key.id())
                            .ok_or_else(|| format!("Contract info for key {key} not found"))?;
                            
                        // Handle update notification
                        match update {
                            UpdateData::State(state) => {
                                let new_state: ChatRoomStateV1 = from_cbor_slice(&state.into_bytes())?;
                                
                                let mut rooms = self.rooms.write();
                                if let Some(room_data) = rooms.map.get_mut(&contract_info.owner_vk) {
                                    let parent_state = &room_data.room_state; // These are the same for the top-level state
                                    let parameters = ChatRoomParametersV1 { owner: contract_info.owner_vk };
                                    room_data.room_state.merge(parent_state, &parameters, &new_state)
                                        .map_err(|e| format!("Failed to merge room state: {}", e))?;
                                    room_data.mark_synced();
                                } else {
                                    warn!("Received state update for unknown room with owner: {:?}", contract_info.owner_vk);
                                }
                            }
                            UpdateData::Delta(delta) => {
                                let new_delta: ChatRoomStateV1Delta = from_cbor_slice(&delta.into_bytes())?;
                                let mut rooms = self.rooms.write();
                                if let Some(room_data) = rooms.map.get_mut(&contract_info.owner_vk) {
                                    let parent_state = &room_data.room_state; // These are the same for the top-level state
                                    let parameters = ChatRoomParametersV1 { owner: contract_info.owner_vk };
                                    room_data.room_state.apply_delta(parent_state, &parameters, &Some(new_delta))
                                        .map_err(|e| format!("Failed to apply delta to room state: {}", e))?;
                                    room_data.mark_synced();
                                } else {
                                    warn!("Received delta update for unknown room with owner: {:?}", contract_info.owner_vk);
                                }
                            }
                            UpdateData::StateAndDelta { .. } => {
                                warn!("Received state and delta update, currently ignored");
                            }
                            UpdateData::RelatedState { .. } => {
                                warn!("Received related state update, currently ignored");
                            }
                            UpdateData::RelatedDelta { .. } => {
                                warn!("Received related delta update, currently ignored");
                            }
                            UpdateData::RelatedStateAndDelta { .. } => {
                                warn!("Received related state and delta update, currently ignored");
                            }
                        }
                    }
                    ContractResponse::UpdateResponse { key, summary } => {}
                    ContractResponse::SubscribeResponse { key, subscribed } => {}
                    _ => {
                        warn!("Unhandled contract response: {:?}", contract_response);
                    }
                }
            }
            _ => {
                warn!("Unhandled API response: {:?}", response);
            }
        }
        Ok(())
    }

}
/*



/// Updates room state from contract state
async fn update_room_state(mut synchronizer: Signal<FreenetSynchronizer>, contract: ContractKey, state: WrappedState) {
    let mut sync = synchronizer.write();
    
    // Find the room with this contract key
    let mut rooms = sync.rooms.write();
    let room_key_opt = rooms.map.iter()
        .find(|(_, data)| data.contract_key == contract)
        .map(|(key, _)| key.clone());
    
    if let Some(room_key) = room_key_opt {
        if let Some(room_data) = rooms.map.get_mut(&room_key) {
            // Try to deserialize the state
            match ChatRoomStateV1::try_from(&state) {
                Ok(new_state) => {
                    info!("Updating room state for: {:?}", room_key);
                    room_data.room_state = new_state;
                    room_data.mark_synced();
                    
                    // Update sync status
                    if room_data.sync_status == RoomSyncStatus::SubscriptionInProgress {
                        room_data.sync_status = RoomSyncStatus::Subscribed;
                    }
                    
                    // Add to subscribed contracts if not already there
                    sync.subscribed_contracts.insert(contract);
                },
                Err(e) => {
                    error!("Failed to deserialize room state: {}", e);
                }
            }
        }
    } else {
        // This might be a response to a pending invitation
        handle_pending_invitation(sync, contract, state);
    }
}

/// Handles pending invitations when receiving contract state
fn handle_pending_invitation(mut sync: std::cell::RefMut<'_, FreenetSynchronizer>, contract: ContractKey, state: ContractState) {
    let mut pending_invites = sync.pending_invites.write();
    
    // Find any pending invitations for this contract
    for (owner_key, pending) in pending_invites.map.iter_mut() {
        let pending_contract = ContractKey::from_verifying_key(owner_key);
        
        if pending_contract == contract {
            info!("Found pending invitation for contract: {:?}", contract);
            
            // Try to deserialize the state
            match ChatRoomStateV1::try_from(&state) {
                Ok(room_state) => {
                    // Create a new room data entry
                    let room_data = RoomData {
                        owner_vk: owner_key.clone(),
                        room_state,
                        self_sk: pending.invitee_signing_key.clone(),
                        contract_key: contract.clone(),
                        sync_status: RoomSyncStatus::Subscribed,
                        last_synced_state: None,
                    };
                    
                    // Add to rooms
                    let mut rooms = sync.rooms.write();
                    rooms.map.insert(owner_key.clone(), room_data);
                    
                    // Update pending status
                    pending.status = crate::invites::PendingRoomStatus::Retrieved;
                    
                    // Add to subscribed contracts
                    sync.subscribed_contracts.insert(contract);
                    
                    info!("Successfully added room from invitation: {:?}", owner_key);
                },
                Err(e) => {
                    error!("Failed to deserialize room state for invitation: {}", e);
                    pending.status = crate::invites::PendingRoomStatus::Error(
                        format!("Failed to deserialize room state: {}", e)
                    );
                }
            }
            
            break;
        }
    }
}

/// Handles successful PUT operations
async fn handle_put_success(mut synchronizer: Signal<FreenetSynchronizer>, contract: ContractKey) {
    let mut sync = synchronizer.write();
    
    // Check if this was a pending operation
    if let Some(op) = sync.pending_puts.remove(&contract) {
        match op {
            PendingOperation::Put { room_key, .. } => {
                info!("PUT operation completed for room: {:?}", room_key);
                
                // Update room status
                let mut rooms = sync.rooms.write();
                if let Some(room_data) = rooms.map.get_mut(&room_key) {
                    if room_data.sync_status == RoomSyncStatus::PutInProgress {
                        room_data.sync_status = RoomSyncStatus::NeedsSync;
                        room_data.mark_synced();
                    }
                }
                
                // Schedule subscription after a short delay
                let mut sync_clone = synchronizer.clone();
                let room_key_clone = room_key.clone();
                spawn_local(async move {
                    // Wait a bit before subscribing to ensure the state is available
                    sleep(Duration::from_millis(1000)).await;
                    
                    match sync_clone.subscribe(&room_key_clone).await {
                        Ok(_) => info!("Scheduled subscription to room after PUT: {:?}", room_key_clone),
                        Err(e) => error!("Failed to subscribe after PUT: {}", e),
                    }
                });
            },
            _ => {}
        }
    }
}

/// Handles successful subscribe operations
async fn handle_subscribe_success(mut synchronizer: Signal<FreenetSynchronizer>, contract: ContractKey) {
    let mut sync = synchronizer.write();
    
    // Add to subscribed contracts
    sync.subscribed_contracts.insert(contract.clone());
    
    // Check if this was a pending operation
    if let Some(op) = sync.pending_puts.remove(&contract) {
        match op {
            PendingOperation::Subscribe { room_key, .. } => {
                info!("Subscribe operation completed for room: {:?}", room_key);
                
                // Update room status
                let mut rooms = sync.rooms.write();
                if let Some(room_data) = rooms.map.get_mut(&room_key) {
                    room_data.sync_status = RoomSyncStatus::Subscribed;
                }
            },
            _ => {}
        }
    }
}

/// Handles API errors
async fn handle_api_error(mut synchronizer: Signal<FreenetSynchronizer>, request: ApiRequest, error: String) {
    let mut sync = synchronizer.write();
    
    match request {
        ApiRequest::PutContractState { contract, .. } => {
            error!("Error putting contract state: {}", error);
            
            // Remove from pending operations
            if let Some(op) = sync.pending_puts.remove(&contract) {
                match op {
                    PendingOperation::Put { room_key, .. } => {
                        // Update room status
                        let mut rooms = sync.rooms.write();
                        if let Some(room_data) = rooms.map.get_mut(&room_key) {
                            room_data.sync_status = RoomSyncStatus::Error(error.clone());
                        }
                    },
                    _ => {}
                }
            }
        },
        ApiRequest::Subscribe { contract } => {
            error!("Error subscribing to contract: {}", error);
            
            // Remove from pending operations
            if let Some(op) = sync.pending_puts.remove(&contract) {
                match op {
                    PendingOperation::Subscribe { room_key, .. } => {
                        // Update room status
                        let mut rooms = sync.rooms.write();
                        if let Some(room_data) = rooms.map.get_mut(&room_key) {
                            room_data.sync_status = RoomSyncStatus::Error(error.clone());
                        }
                    },
                    _ => {}
                }
            }
        },
        ApiRequest::GetContractState { contract } => {
            error!("Error getting contract state: {}", error);
            
            // Check if this was for a pending invitation
            let mut pending_invites = sync.pending_invites.write();
            for (owner_key, pending) in pending_invites.map.iter_mut() {
                let pending_contract = ContractKey::from_verifying_key(owner_key);
                
                if pending_contract == contract {
                    pending.status = crate::invites::PendingRoomStatus::Error(
                        format!("Failed to get room state: {}", error)
                    );
                    break;
                }
            }
        },
        _ => {
            error!("Unhandled API error for request {:?}: {}", request, error);
        }
    }
}

*/
