//! Y.js WebSocket client for real-time notebook document synchronization

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use nbformat::v4::Output;
use reqwest::Client as HttpClient;
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use url::Url;
use yrs::encoding::varint::VarInt;
use yrs::encoding::write::Write;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{ArrayRef, Doc, ReadTxn, StateVector, Transact, Update};

use super::output_conversion::{update_cell_execution_count, update_cell_outputs};

#[derive(Debug, Deserialize)]
struct FileIdResponse {
    id: String,
    #[allow(dead_code)]
    path: String,
}

/// Y.js document client for syncing notebook changes with Jupyter Server
pub struct YDocClient {
    doc: Doc,
    ws: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    file_id: String,
    #[allow(dead_code)]
    server_url: String,
    #[allow(dead_code)]
    notebook_path: String,
    /// Track the document state when we last synced, so we only send changes
    last_state: StateVector,
}

impl YDocClient {
    /// Connect to Y.js room for the given notebook
    pub async fn connect(
        server_url: String,
        token: String,
        notebook_path: String,
    ) -> Result<Self> {
        eprintln!("DEBUG: Connecting to Y.js document for: {}", notebook_path);

        // Step 1: Get file ID from FileID API
        let file_id = Self::get_file_id(&server_url, &token, &notebook_path)
            .await
            .context("Failed to get file ID from FileID API")?;

        eprintln!("  File ID: {}", file_id);

        // Step 2: Connect to room WebSocket
        let ws_url = Self::build_room_ws_url(&server_url, &file_id, &token)?;
        eprintln!("  Room WebSocket URL: {}", ws_url);

        let (ws_stream, _) = connect_async(&ws_url)
            .await
            .context("Failed to connect to Y.js room WebSocket")?;

        eprintln!("  ✓ Connected to Y.js room");

        // Step 3: Initialize Y.js document
        let doc = Doc::new();

        let mut client = Self {
            doc,
            ws: ws_stream,
            file_id,
            server_url,
            notebook_path,
            last_state: StateVector::default(),
        };

        // Step 4: Perform Y.js sync handshake with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client.sync_handshake()
        ).await {
            Ok(Ok(_)) => {
                eprintln!("  ✓ Y.js sync handshake completed");
                Ok(client)
            }
            Ok(Err(e)) => {
                Err(e).context("Failed to perform Y.js sync handshake")
            }
            Err(_) => {
                Err(anyhow::anyhow!("Y.js sync handshake timed out after 3 seconds"))
            }
        }
    }

    /// Get unique file ID for notebook path via FileID API
    async fn get_file_id(server_url: &str, token: &str, notebook_path: &str) -> Result<String> {
        let url = format!("{}/api/fileid/index", server_url);

        let http_client = HttpClient::new();
        let response = http_client
            .post(&url)
            .query(&[("path", notebook_path)])
            .header("Authorization", format!("token {}", token))
            .send()
            .await
            .context("Failed to call FileID API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "FileID API request failed with status {}: {}. \
                 Make sure jupyter-server-documents is installed: \
                 pip install jupyter-server-documents",
                status,
                error_text
            );
        }

        let file_id_response: FileIdResponse = response
            .json()
            .await
            .context("Failed to parse FileID API response")?;

        Ok(file_id_response.id)
    }

    /// Build WebSocket URL for Y.js room
    fn build_room_ws_url(server_url: &str, file_id: &str, token: &str) -> Result<String> {
        // Parse base URL to extract host and port
        let base_url = Url::parse(server_url).context("Invalid server URL")?;

        let host = base_url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("No host in server URL"))?;

        let port = base_url.port().unwrap_or(if base_url.scheme() == "https" {
            443
        } else {
            8888
        });

        // Build WebSocket URL with json:notebook: prefix
        let ws_scheme = if base_url.scheme() == "https" {
            "wss"
        } else {
            "ws"
        };

        let ws_url = format!(
            "{}://{}:{}/api/collaboration/room/json:notebook:{}?token={}",
            ws_scheme, host, port, file_id, token
        );

        Ok(ws_url)
    }

    /// Perform Y.js sync protocol handshake
    async fn sync_handshake(&mut self) -> Result<()> {
        // Step 1: Send our state vector (SyncStep1)
        // pycrdt message format: [YMessageType.SYNC, YSyncMessageType.SYNC_STEP1, length, data]
        let state_vector = self.doc.transact().state_vector();
        let sv_bytes = state_vector.encode_v1();

        // Build message: [SYNC=0, SYNC_STEP1=0, length_varint, state_vector_bytes]
        let mut msg: Vec<u8> = Vec::new();
        msg.write_u8(0); // YMessageType.SYNC
        msg.write_u8(0); // YSyncMessageType.SYNC_STEP1
        (sv_bytes.len() as u32).write(&mut msg); // Length as varint
        msg.extend_from_slice(&sv_bytes);

        eprintln!("    Sending SyncStep1: {} bytes total (sv: {} bytes)", msg.len(), sv_bytes.len());
        eprintln!("    Message hex: {:02x?}", &msg);

        self.ws
            .send(Message::Binary(msg))
            .await
            .context("Failed to send SyncStep1")?;

        eprintln!("    Sent SyncStep1");

        // Step 2: Receive messages until we get SyncStep2
        // We may receive SyncStep1 from server first, then SyncStep2
        let mut received_sync_step2 = false;

        while !received_sync_step2 {
            eprintln!("    Waiting for message from server...");
            let msg_result = self.ws.next().await;
            eprintln!("    Received message result: {:?}", msg_result.is_some());

            if msg_result.is_none() {
                return Err(anyhow::anyhow!("WebSocket closed during handshake - connection terminated by server"));
            }

            let msg = msg_result.unwrap();

            if let Err(e) = &msg {
                return Err(anyhow::anyhow!("WebSocket error during handshake: {}", e));
            }

            let msg = msg?;
            eprintln!("    Message type: {:?}", msg);

            match msg {
                Message::Binary(data) => {
                    if data.len() < 2 {
                        eprintln!("    Received message too short: {} bytes", data.len());
                        continue;
                    }

                    // Parse: [YMessageType, YSyncMessageType, length_varint, payload]
                    let y_msg_type = data[0];
                    let sync_msg_type = data[1];
                    let payload_with_length = &data[2..];

                    eprintln!("    Received message: y_type={}, sync_type={}, data_len={}",
                              y_msg_type, sync_msg_type, payload_with_length.len());

                    // We only handle SYNC messages (type 0)
                    if y_msg_type != 0 {
                        eprintln!("    Ignoring non-SYNC message type: {}", y_msg_type);
                        continue;
                    }

                    // Decode the length prefix and get actual payload
                    let mut decoder = yrs::encoding::read::Cursor::new(payload_with_length);
                    let payload_length = u32::read(&mut decoder)
                        .context("Failed to read payload length")?;

                    let payload_start = decoder.next;
                    let payload = &payload_with_length[payload_start..payload_start + payload_length as usize];

                    eprintln!("    Payload length: {}, actual payload: {} bytes", payload_length, payload.len());

                    match sync_msg_type {
                        0 => {
                            // SyncStep1 from server - send SyncStep2 in response
                            eprintln!("    Processing SyncStep1 from server");

                            let server_state = StateVector::decode_v1(payload)
                                .context("Failed to decode server state vector")?;

                            eprintln!("    Server state decoded, generating updates");

                            // Get updates since server's state
                            let txn = self.doc.transact();
                            let update = txn.encode_state_as_update_v1(&server_state);

                            // Build response: [SYNC=0, SYNC_STEP2=1, length_varint, update_bytes]
                            let mut response: Vec<u8> = Vec::new();
                            response.write_u8(0); // YMessageType.SYNC
                            response.write_u8(1); // YSyncMessageType.SYNC_STEP2
                            (update.len() as u32).write(&mut response); // Length as varint
                            response.extend_from_slice(&update);

                            self.ws
                                .send(Message::Binary(response))
                                .await
                                .context("Failed to send SyncStep2")?;

                            eprintln!("    Sent SyncStep2 ({} bytes update)", update.len());
                        }
                        1 => {
                            // SyncStep2 from server - apply updates
                            eprintln!("    Processing SyncStep2 from server");

                            let update =
                                Update::decode_v1(payload).context("Failed to decode update")?;

                            // Apply the update and get state vector in same transaction scope
                            {
                                let mut txn = self.doc.transact_mut();
                                let _ = txn.apply_update(update);
                            } // Transaction dropped here

                            received_sync_step2 = true;
                            eprintln!("    Applied server updates, sync complete!");

                            // Now safe to create new transaction for state vector
                            self.last_state = self.doc.transact().state_vector();
                            eprintln!("    Saved document state vector for tracking changes");

                            // DEBUG: Inspect the document structure we received
                            eprintln!("\n  === INSPECTING Y.JS DOCUMENT STRUCTURE ===");
                            Self::inspect_document(&self.doc);
                            eprintln!("  === END DOCUMENT STRUCTURE ===\n");
                        }
                        2 => {
                            // Regular update message - apply it
                            eprintln!("    Processing update message");

                            let update =
                                Update::decode_v1(payload).context("Failed to decode update")?;

                            let mut txn = self.doc.transact_mut();
                            let _ = txn.apply_update(update);
                        }
                        _ => {
                            eprintln!("    Received unknown sync message type: {}", sync_msg_type);
                        }
                    }
                }
                Message::Text(text) => {
                    eprintln!("    Received text message: {}", text);
                }
                Message::Ping(_) => {
                    eprintln!("    Received ping");
                }
                Message::Pong(_) => {
                    eprintln!("    Received pong");
                }
                Message::Close(frame) => {
                    eprintln!("    Received close frame: {:?}", frame);
                    return Err(anyhow::anyhow!("Server closed WebSocket connection during handshake"));
                }
                Message::Frame(_) => {
                    eprintln!("    Received raw frame");
                }
            }
        }

        Ok(())
    }

    /// Update cell outputs in the Y.js document
    pub fn update_cell_outputs(&mut self, cell_index: usize, outputs: Vec<Output>) -> Result<()> {
        use yrs::{Array, Map};

        eprintln!("    DEBUG: Starting update_cell_outputs for cell {}", cell_index);

        // Get cells array from document (outside transaction)
        let cells_array: ArrayRef = self.doc.get_or_insert_array("cells");
        eprintln!("    DEBUG: Got cells array reference");

        // Create transaction after getting the array reference
        let mut txn = self.doc.transact_mut();
        eprintln!("    DEBUG: Created transaction");

        let cells_len = cells_array.len(&txn);
        eprintln!("    DEBUG: cells array length: {}", cells_len);

        // Inspect the cell structure before modifying
        if let Some(cell_val) = cells_array.get(&txn, cell_index as u32) {
            if let Ok(cell_map) = cell_val.cast::<yrs::MapRef>() {
                eprintln!("    DEBUG: Cell {} keys: {:?}", cell_index,
                    cell_map.keys(&txn).collect::<Vec<_>>());

                if let Some(outputs_val) = cell_map.get(&txn, "outputs") {
                    if let Ok(outputs_arr) = outputs_val.cast::<yrs::ArrayRef>() {
                        eprintln!("    DEBUG: Current outputs array length: {}", outputs_arr.len(&txn));
                    }
                }
            }
        }

        // Update outputs using helper function
        eprintln!("    DEBUG: Calling update_cell_outputs helper");
        update_cell_outputs(&mut txn, &cells_array, cell_index, &outputs)
            .context("Failed to update cell outputs")?;

        eprintln!("    DEBUG: update_cell_outputs completed");

        Ok(())
    }

    /// Update cell execution_count in the Y.js document
    pub fn update_cell_execution_count(
        &mut self,
        cell_index: usize,
        execution_count: Option<i64>,
    ) -> Result<()> {
        eprintln!("    DEBUG: Starting update_cell_execution_count for cell {}", cell_index);

        // Get cells array from document (outside transaction)
        let cells_array: ArrayRef = self.doc.get_or_insert_array("cells");
        eprintln!("    DEBUG: Got cells array reference for execution_count");

        // Create transaction after getting the array reference
        let mut txn = self.doc.transact_mut();
        eprintln!("    DEBUG: Created transaction for execution_count");

        // Update execution_count using helper function
        update_cell_execution_count(&mut txn, &cells_array, cell_index, execution_count)
            .context("Failed to update execution count")?;

        eprintln!("    DEBUG: update_cell_execution_count completed");

        Ok(())
    }

    /// Synchronize changes to the server (broadcast updates)
    pub async fn sync(&mut self) -> Result<()> {
        // Get ONLY the changes since last sync, not the entire document
        let txn = self.doc.transact();

        eprintln!("  DEBUG: Current state vector: {:?}", txn.state_vector());
        eprintln!("  DEBUG: Last state vector: {:?}", self.last_state);

        let update = txn.encode_state_as_update_v1(&self.last_state);

        eprintln!("  Encoding update since last state: {} bytes", update.len());
        eprintln!("  Update hex (first 100 bytes): {:02x?}", &update[..update.len().min(100)]);

        // Check if there are actually any changes
        if update.is_empty() || update == vec![0, 0] {
            eprintln!("  No changes to sync (empty update)");
            return Ok(());
        }

        // Build update message: [SYNC=0, SYNC_UPDATE=2, length_varint, update_bytes]
        let mut msg: Vec<u8> = Vec::new();
        msg.write_u8(0); // YMessageType.SYNC
        msg.write_u8(2); // YSyncMessageSubtype.SYNC_UPDATE
        (update.len() as u32).write(&mut msg); // Length as varint
        msg.extend_from_slice(&update);

        eprintln!("  Sending Y.js update: {} bytes (update: {} bytes)", msg.len(), update.len());
        eprintln!("  Full message hex (first 100 bytes): {:02x?}", &msg[..msg.len().min(100)]);

        self.ws
            .send(Message::Binary(msg))
            .await
            .context("Failed to send update to server")?;

        eprintln!("  ✓ Sent Y.js update to server");

        // Update our state vector to the current state
        self.last_state = txn.state_vector();
        eprintln!("  DEBUG: Updated last_state to: {:?}", self.last_state);

        // Flush to ensure message is sent
        self.ws.flush().await.context("Failed to flush WebSocket")?;

        Ok(())
    }

    /// Inspect and print Y.js document structure
    fn inspect_document(doc: &Doc) {
        let txn = doc.transact();
        eprintln!("  State vector: {:?}", txn.state_vector());
        eprintln!("  Document synced successfully!");
    }

    /// Close the WebSocket connection
    pub async fn close(mut self) -> Result<()> {
        self.ws.close(None).await.context("Failed to close WebSocket")?;
        Ok(())
    }
}
