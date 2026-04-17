//! cTrader OpenAPI Client
//!
//! Real TCP/TLS client for Spotware's cTrader Open API.
//!
//! # Architecture
//! - Owns an internal multi-thread `tokio::Runtime` so sync callers can use it.
//! - Background **reader task**: continuously reads frames from the TLS stream,
//!   dispatches spot events to shared tick state, and resolves pending request
//!   futures.
//! - Background **writer task**: drains a channel queue and writes encoded
//!   frames to the TLS stream.
//! - Background **heartbeat task**: sends `ProtoHeartbeatEvent` every 25 s.
//! - `connect_blocking()`: connect → app auth → account auth → symbol list →
//!   subscribe spots.
//! - `reconcile_blocking()`: reconcile open positions/orders after a restart.
//! - Implements the sync `Broker` trait via `rt.block_on(...)`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::BytesMut;
use prost::Message as ProstMessage;
use rust_decimal::Decimal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::TlsConnector;
use tracing::{debug, error, info, warn};

use crate::codec::{decode_frame, encode_message, encode_proto_message};
use crate::error::BrokerError;
use crate::proto::*;
use crate::traits::Broker;
use crate::types::*;

// ── Constants ────────────────────────────────────────────────────────────────

const LIVE_HOST: &str = "live.ctraderapi.com";
const DEMO_HOST: &str = "demo.ctraderapi.com";
const CTRADER_PORT: u16 = 5035;
const HEARTBEAT_SECS: u64 = 25;
const REQUEST_TIMEOUT_SECS: u64 = 10;
const READ_BUF_SIZE: usize = 64 * 1024; // 64 KB

// Proto payload type constants (from ProtoOaPayloadType enum)
const PT_HEARTBEAT: u32 = 51;
const PT_APP_AUTH_REQ: u32 = 2100;
const PT_ACC_AUTH_REQ: u32 = 2102;
const PT_TRADER_REQ: u32 = 2121;
const PT_TRADER_RES: u32 = 2122;
const PT_RECONCILE_REQ: u32 = 2124;
const PT_SYMBOLS_LIST_REQ: u32 = 2114;
const PT_NEW_ORDER_REQ: u32 = 2106;
const PT_EXECUTION_EVENT: u32 = 2126;
const PT_SUBSCRIBE_SPOTS_REQ: u32 = 2127;
const PT_SPOT_EVENT: u32 = 2131;
const PT_AMEND_SLTP_REQ: u32 = 2110;
const PT_CLOSE_POSITION_REQ: u32 = 2111;
const PT_ERROR_RES: u32 = 2142;
const PT_GET_ACCOUNTS_REQ: u32 = 2149;

// cTrader price scaling: prices are integers in 1/100000 units
const PRICE_SCALE: u64 = 100_000;
// Balance is in cents (1/100 of account currency unit)
const MONEY_SCALE: i64 = 100;
// Volume is in 0.01 lot units (1000 = 10.00 lots in protocol; 100 = 1 lot)
const VOLUME_SCALE: i64 = 100;

// ── Config ───────────────────────────────────────────────────────────────────

/// Configuration for the cTrader TCP/TLS client.
#[derive(Debug, Clone)]
pub struct CtraderConfig {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: Option<String>,
    pub ctid_account_id: Option<i64>,
    /// `true` → connect to `demo.ctraderapi.com` (safe default).
    /// `false` → connect to `live.ctraderapi.com`.
    pub is_demo: bool,
}

impl CtraderConfig {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self {
            client_id,
            client_secret,
            access_token: None,
            ctid_account_id: None,
            is_demo: true, // default to demo for safety
        }
    }

    pub fn with_account(mut self, access_token: String, ctid_account_id: i64) -> Self {
        self.access_token = Some(access_token);
        self.ctid_account_id = Some(ctid_account_id);
        self
    }

    /// Switch to live server. Call only when ready to trade real money.
    pub fn live(mut self) -> Self {
        self.is_demo = false;
        self
    }

    fn host(&self) -> &'static str {
        if self.is_demo {
            DEMO_HOST
        } else {
            LIVE_HOST
        }
    }
}

// ── Inner shared state ────────────────────────────────────────────────────────

/// Pending request: stores the expected response payload type and a oneshot
/// channel to deliver the raw payload bytes (or an error).
type PendingEntry = oneshot::Sender<Result<prost::bytes::Bytes, BrokerError>>;

struct CtraderInner {
    connected: AtomicBool,
    authenticated: AtomicBool,
    /// Channel to the writer task.  None when disconnected.
    write_tx: Mutex<Option<mpsc::UnboundedSender<prost::bytes::Bytes>>>,
    /// In-flight requests keyed by `client_msg_id`.
    pending: Mutex<HashMap<String, PendingEntry>>,
    /// Latest account snapshot.
    account_info: Mutex<Option<BrokerAccountInfo>>,
    /// our_symbol (e.g. "EURUSD") → cTrader symbol_id
    symbols: Mutex<HashMap<String, i64>>,
    /// cTrader symbol_id → our_symbol
    rev_symbols: Mutex<HashMap<i64, String>>,
    /// Latest tick per symbol.
    ticks: Mutex<HashMap<String, Tick>>,
    /// Monotonic request counter for client_msg_id generation.
    next_id: Mutex<u32>,
    /// Account ID received during auth.
    ctid_account_id: Mutex<Option<i64>>,
}

impl CtraderInner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            connected: AtomicBool::new(false),
            authenticated: AtomicBool::new(false),
            write_tx: Mutex::new(None),
            pending: Mutex::new(HashMap::new()),
            account_info: Mutex::new(None),
            symbols: Mutex::new(HashMap::new()),
            rev_symbols: Mutex::new(HashMap::new()),
            ticks: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            ctid_account_id: Mutex::new(None),
        })
    }

    fn next_msg_id(&self) -> String {
        let mut id = self.next_id.lock().unwrap();
        let n = *id;
        *id = id.wrapping_add(1);
        n.to_string()
    }

    /// Queue an encoded frame for the writer task.
    fn queue_frame(&self, frame: prost::bytes::Bytes) -> Result<(), BrokerError> {
        let guard = self.write_tx.lock().unwrap();
        guard
            .as_ref()
            .ok_or_else(|| BrokerError::Connection("Not connected".into()))?
            .send(frame)
            .map_err(|_| BrokerError::Connection("Writer task closed".into()))
    }

    /// Encode and queue a message.
    fn send_msg<T: ProstMessage>(
        &self,
        payload_type: u32,
        msg: &T,
        msg_id: Option<String>,
    ) -> Result<(), BrokerError> {
        let frame = encode_message(payload_type, msg, msg_id)
            .map_err(|e| BrokerError::Protocol(e.to_string()))?;
        self.queue_frame(frame.freeze())
    }

    /// Deliver a response payload to the waiting request handler.
    fn dispatch_response(&self, msg_id: &str, payload: prost::bytes::Bytes) {
        if let Some(tx) = self.pending.lock().unwrap().remove(msg_id) {
            let _ = tx.send(Ok(payload));
        }
    }

    /// Deliver an error to the waiting request handler (if any).
    fn dispatch_error_to(&self, msg_id: Option<&str>, err: BrokerError) {
        if let Some(id) = msg_id {
            if let Some(tx) = self.pending.lock().unwrap().remove(id) {
                let _ = tx.send(Err(err));
            }
        }
    }
}

// ── Helper conversions ────────────────────────────────────────────────────────

/// Lots (Decimal, e.g. 0.10) → cTrader volume (i64, e.g. 1000 = 10.00 units).
/// cTrader volume unit = 0.01 lots. 1 standard lot = 100_000 units = volume 100.
/// So lots * 100 = volume.
fn lots_to_volume(lots: &Decimal) -> i64 {
    (lots * Decimal::from(VOLUME_SCALE))
        .to_string()
        .parse::<f64>()
        .map(|v| v.round() as i64)
        .unwrap_or(0)
}

/// cTrader volume → Decimal lots.
fn volume_to_lots(volume: i64) -> Decimal {
    Decimal::from(volume) / Decimal::from(VOLUME_SCALE)
}

/// cTrader scaled price (u64, 1/100000) → Decimal.
fn scaled_to_decimal(scaled: u64) -> Decimal {
    Decimal::from(scaled) / Decimal::from(PRICE_SCALE)
}

/// cTrader balance (i64 cents) → Decimal USD.
fn cents_to_decimal(cents: i64) -> Decimal {
    Decimal::from(cents) / Decimal::from(MONEY_SCALE)
}

/// Normalise a cTrader symbol name ("EUR/USD", "EURUSD") to our internal form ("EURUSD").
fn normalise_symbol(name: &str) -> String {
    name.replace('/', "").to_uppercase()
}

// ── Background tasks ──────────────────────────────────────────────────────────

/// Writer task: takes pre-encoded frames from `rx` and writes them to the TLS stream.
async fn writer_task(
    mut write_half: tokio::io::WriteHalf<tokio_rustls::client::TlsStream<TcpStream>>,
    mut rx: mpsc::UnboundedReceiver<prost::bytes::Bytes>,
) {
    while let Some(frame) = rx.recv().await {
        if let Err(e) = write_half.write_all(&frame).await {
            error!("cTrader write error: {e}");
            break;
        }
    }
    debug!("cTrader writer task exiting");
}

/// Heartbeat task: sends a ProtoHeartbeatEvent every `HEARTBEAT_SECS` seconds.
async fn heartbeat_task(inner: Arc<CtraderInner>) {
    let interval = Duration::from_secs(HEARTBEAT_SECS);
    loop {
        tokio::time::sleep(interval).await;
        if !inner.connected.load(Ordering::Acquire) {
            break;
        }
        let hb = ProtoHeartbeatEvent { payload_type: None };
        let wrapper = ProtoMessage {
            payload_type: PT_HEARTBEAT,
            payload: {
                let mut b = Vec::new();
                hb.encode(&mut b).ok();
                Some(prost::bytes::Bytes::from(b))
            },
            client_msg_id: None,
        };
        if let Ok(frame) = encode_proto_message(&wrapper) {
            let _ = inner.queue_frame(frame.freeze());
        }
        debug!("cTrader heartbeat sent");
    }
    debug!("cTrader heartbeat task exiting");
}

/// Reader task: continuously reads frames from TLS, dispatches events/responses.
async fn reader_task(
    mut read_half: tokio::io::ReadHalf<tokio_rustls::client::TlsStream<TcpStream>>,
    inner: Arc<CtraderInner>,
) {
    let mut buf = BytesMut::with_capacity(READ_BUF_SIZE);
    let mut tmp = vec![0u8; READ_BUF_SIZE];

    loop {
        match read_half.read(&mut tmp).await {
            Ok(0) => {
                info!("cTrader server closed connection");
                break;
            }
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);
                // Decode all complete frames in the buffer
                loop {
                    match decode_frame(&mut buf) {
                        Ok(Some(msg)) => handle_incoming(msg, &inner),
                        Ok(None) => break, // need more data
                        Err(e) => {
                            error!("cTrader frame decode error: {e}");
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                error!("cTrader read error: {e}");
                break;
            }
        }
    }

    inner.connected.store(false, Ordering::Release);
    inner.authenticated.store(false, Ordering::Release);
    // Fail all pending requests
    let mut pending = inner.pending.lock().unwrap();
    for (_, tx) in pending.drain() {
        let _ = tx.send(Err(BrokerError::Connection("Connection lost".into())));
    }
    debug!("cTrader reader task exiting");
}

/// Dispatch one decoded `ProtoMessage` to the right handler.
fn handle_incoming(msg: ProtoMessage, inner: &Arc<CtraderInner>) {
    let pt = msg.payload_type;
    let payload = msg.payload.unwrap_or_default();
    let msg_id = msg.client_msg_id;

    match pt {
        // ── Spot event → update tick ──────────────────────────────────────
        PT_SPOT_EVENT => {
            if let Ok(ev) = ProtoOaSpotEvent::decode(payload.as_ref()) {
                let symbol_id = ev.symbol_id;
                let bid = ev.bid.map(scaled_to_decimal).unwrap_or(Decimal::ZERO);
                let ask = ev.ask.map(scaled_to_decimal).unwrap_or(Decimal::ZERO);
                let ts = chrono::Utc::now().timestamp();

                let rev = inner.rev_symbols.lock().unwrap();
                if let Some(sym) = rev.get(&symbol_id) {
                    let tick = Tick {
                        symbol: sym.clone(),
                        bid,
                        ask,
                        timestamp: ts,
                    };
                    inner.ticks.lock().unwrap().insert(sym.clone(), tick);
                    debug!("Tick {sym}: bid={bid} ask={ask}");
                }
            }
        }

        // ── Execution event → resolve pending order request ───────────────
        PT_EXECUTION_EVENT => {
            if let Some(id) = msg_id.as_deref() {
                inner.dispatch_response(id, payload.clone());
            }
            // Also handle unsolicited fills (SL/TP hits) — just log for now.
            if msg_id.is_none() {
                if let Ok(ev) = ProtoOaExecutionEvent::decode(payload.as_ref()) {
                    if let Some(deal) = &ev.deal {
                        debug!(
                            "Unsolicited execution: position={:?} price={:?}",
                            ev.position.as_ref().map(|p| p.position_id),
                            deal.execution_price
                        );
                    }
                }
            }
        }

        // ── Trader info response ──────────────────────────────────────────
        PT_TRADER_RES => {
            if let Ok(res) = ProtoOaTraderRes::decode(payload.as_ref()) {
                let t = &res.trader;
                let info = BrokerAccountInfo {
                    account_id: t.ctid_trader_account_id,
                    balance: cents_to_decimal(t.balance),
                    equity: cents_to_decimal(t.balance), // equity updated separately
                    margin_used: Decimal::ZERO,
                    free_margin: cents_to_decimal(t.balance),
                    currency: "USD".to_string(),
                };
                *inner.account_info.lock().unwrap() = Some(info);
                info!("Account balance: {}", cents_to_decimal(t.balance));
            }
            if let Some(id) = msg_id.as_deref() {
                inner.dispatch_response(id, payload);
            }
        }

        // ── Error response ────────────────────────────────────────────────
        PT_ERROR_RES => {
            if let Ok(err_res) = ProtoOaErrorRes::decode(payload.as_ref()) {
                let reason = err_res
                    .description
                    .as_deref()
                    .unwrap_or(&err_res.error_code);
                warn!("cTrader error: {} — {}", err_res.error_code, reason);
                let err = BrokerError::Protocol(format!("{}: {}", err_res.error_code, reason));
                inner.dispatch_error_to(msg_id.as_deref(), err);
            }
        }

        // ── All other responses with a client_msg_id → deliver raw payload ─
        _ => {
            if let Some(id) = msg_id.as_deref() {
                inner.dispatch_response(id, payload);
            }
        }
    }
}

// ── Connection / auth flow ────────────────────────────────────────────────────

/// Build a TLS client config using Mozilla root certificates.
fn build_tls_config() -> Arc<rustls::ClientConfig> {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth(),
    )
}

/// Send a typed request, wait for a typed response, with a timeout.
async fn request<Req, Res>(
    inner: &Arc<CtraderInner>,
    payload_type: u32,
    req: &Req,
    operation: &str,
) -> Result<Res, BrokerError>
where
    Req: ProstMessage,
    Res: ProstMessage + Default,
{
    let msg_id = inner.next_msg_id();
    let (tx, rx) = oneshot::channel();
    inner.pending.lock().unwrap().insert(msg_id.clone(), tx);
    inner.send_msg(payload_type, req, Some(msg_id))?;

    let payload = tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), rx)
        .await
        .map_err(|_| BrokerError::Timeout {
            operation: operation.to_string(),
        })?
        .map_err(|_| BrokerError::Connection("Response channel dropped".into()))??;

    Res::decode(payload.as_ref()).map_err(|e| BrokerError::Protocol(e.to_string()))
}

/// Establish TLS connection, authenticate, fetch symbol list, subscribe to spots.
async fn connect_and_auth(
    inner: Arc<CtraderInner>,
    config: CtraderConfig,
) -> Result<(), BrokerError> {
    let host = config.host();
    info!(
        "Connecting to cTrader {} at {}:{}",
        if config.is_demo { "demo" } else { "live" },
        host,
        CTRADER_PORT
    );

    // ── TCP + TLS ──────────────────────────────────────────────────────────
    let tcp = TcpStream::connect((host, CTRADER_PORT))
        .await
        .map_err(|e| BrokerError::Connection(format!("TCP connect failed: {e}")))?;

    let tls_config = build_tls_config();
    let connector = TlsConnector::from(tls_config);
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| BrokerError::Connection(format!("Invalid host: {host}")))?;
    let tls_stream = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| BrokerError::Connection(format!("TLS handshake failed: {e}")))?;

    info!("TLS connection established to {host}");

    let (read_half, write_half) = tokio::io::split(tls_stream);

    // ── Writer task ────────────────────────────────────────────────────────
    let (write_tx, write_rx) = mpsc::unbounded_channel::<prost::bytes::Bytes>();
    {
        let mut guard = inner.write_tx.lock().unwrap();
        *guard = Some(write_tx);
    }
    tokio::spawn(writer_task(write_half, write_rx));

    // ── Reader task ────────────────────────────────────────────────────────
    let inner_reader = Arc::clone(&inner);
    tokio::spawn(reader_task(read_half, inner_reader));

    inner.connected.store(true, Ordering::Release);

    // ── Heartbeat task ─────────────────────────────────────────────────────
    let inner_hb = Arc::clone(&inner);
    tokio::spawn(heartbeat_task(inner_hb));

    // ── Application authentication ─────────────────────────────────────────
    let app_auth = ProtoOaApplicationAuthReq {
        payload_type: None,
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
    };
    let _: ProtoOaApplicationAuthRes =
        request(&inner, PT_APP_AUTH_REQ, &app_auth, "application auth").await?;
    info!("Application authenticated");

    // ── Account authentication ─────────────────────────────────────────────
    let access_token = config
        .access_token
        .as_deref()
        .ok_or_else(|| BrokerError::AuthFailed("No access token configured".into()))?;
    let ctid = config
        .ctid_account_id
        .ok_or_else(|| BrokerError::AuthFailed("No ctid_account_id configured".into()))?;

    let acc_auth = ProtoOaAccountAuthReq {
        payload_type: None,
        ctid_trader_account_id: ctid,
        access_token: access_token.to_string(),
    };
    let _: ProtoOaAccountAuthRes =
        request(&inner, PT_ACC_AUTH_REQ, &acc_auth, "account auth").await?;
    *inner.ctid_account_id.lock().unwrap() = Some(ctid);
    inner.authenticated.store(true, Ordering::Release);
    info!("Account {ctid} authenticated");

    // ── Fetch account balance ──────────────────────────────────────────────
    let trader_req = ProtoOaTraderReq {
        payload_type: None,
        ctid_trader_account_id: ctid,
    };
    let _: ProtoOaTraderRes = request(&inner, PT_TRADER_REQ, &trader_req, "trader info").await?;

    // ── Fetch symbol list ──────────────────────────────────────────────────
    let sym_req = ProtoOaSymbolsListReq {
        payload_type: None,
        ctid_trader_account_id: ctid,
        include_archived_symbols: Some(false),
    };
    let sym_res: ProtoOaSymbolsListRes =
        request(&inner, PT_SYMBOLS_LIST_REQ, &sym_req, "symbols list").await?;

    {
        let mut syms = inner.symbols.lock().unwrap();
        let mut rev = inner.rev_symbols.lock().unwrap();
        for s in &sym_res.symbol {
            if let Some(name) = &s.symbol_name {
                let our_name = normalise_symbol(name);
                syms.insert(our_name.clone(), s.symbol_id);
                rev.insert(s.symbol_id, our_name.clone());
                debug!("Symbol: {our_name} → id={}", s.symbol_id);
            }
        }
        info!("Loaded {} symbols", syms.len());
    }

    // ── Subscribe to spots for configured symbols ──────────────────────────
    let symbol_ids: Vec<i64> = {
        let syms = inner.symbols.lock().unwrap();
        // Subscribe to the most common FX pairs + gold
        ["EURUSD", "GBPUSD", "USDJPY", "XAUUSD", "USDCAD", "AUDUSD"]
            .iter()
            .filter_map(|s| syms.get(*s).copied())
            .collect()
    };

    if !symbol_ids.is_empty() {
        let sub_req = ProtoOaSubscribeSpotsReq {
            payload_type: None,
            ctid_trader_account_id: ctid,
            symbol_id: symbol_ids.clone(),
            subscribe_to_spot_timestamp: Some(false),
        };
        let _: ProtoOaSubscribeSpotsRes =
            request(&inner, PT_SUBSCRIBE_SPOTS_REQ, &sub_req, "subscribe spots").await?;
        info!("Subscribed to {} spot streams", symbol_ids.len());
    }

    info!("cTrader client ready");
    Ok(())
}

/// Connect with app-auth only (no account auth, no symbols, no spots).
/// Used for account listing during the OAuth flow.
async fn connect_app_only(
    inner: Arc<CtraderInner>,
    config: CtraderConfig,
) -> Result<(), BrokerError> {
    let host = config.host();
    info!("Connecting to cTrader {} for account listing", host);

    let tcp = TcpStream::connect((host, CTRADER_PORT))
        .await
        .map_err(|e| BrokerError::Connection(format!("TCP connect failed: {e}")))?;

    let tls_config = build_tls_config();
    let connector = TlsConnector::from(tls_config);
    let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
        .map_err(|_| BrokerError::Connection(format!("Invalid host: {host}")))?;
    let tls_stream = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| BrokerError::Connection(format!("TLS handshake failed: {e}")))?;

    let (read_half, write_half) = tokio::io::split(tls_stream);

    let (write_tx, write_rx) = mpsc::unbounded_channel::<prost::bytes::Bytes>();
    {
        *inner.write_tx.lock().unwrap() = Some(write_tx);
    }
    tokio::spawn(writer_task(write_half, write_rx));
    tokio::spawn(reader_task(read_half, Arc::clone(&inner)));
    inner.connected.store(true, Ordering::Release);
    tokio::spawn(heartbeat_task(Arc::clone(&inner)));

    // App auth only
    let app_auth = ProtoOaApplicationAuthReq {
        payload_type: None,
        client_id: config.client_id.clone(),
        client_secret: config.client_secret.clone(),
    };
    let _: ProtoOaApplicationAuthRes =
        request(&inner, PT_APP_AUTH_REQ, &app_auth, "application auth").await?;
    info!("Application authenticated (account listing mode)");

    Ok(())
}

/// Query account list by access token (requires app-auth connection).
async fn do_list_accounts(
    inner: Arc<CtraderInner>,
    access_token: &str,
) -> Result<Vec<crate::auth::TradingAccount>, BrokerError> {
    let req = ProtoOaGetAccountListByAccessTokenReq {
        payload_type: None,
        access_token: access_token.to_string(),
    };
    let res: ProtoOaGetAccountListByAccessTokenRes =
        request(&inner, PT_GET_ACCOUNTS_REQ, &req, "get account list").await?;

    let accounts = res
        .ctid_trader_account
        .iter()
        .map(|a| crate::auth::TradingAccount {
            ctid_trader_account_id: a.ctid_trader_account_id,
            is_live: a.is_live.unwrap_or(false),
            trader_login: a.trader_login,
            broker_name: a.broker_title_short.clone(),
        })
        .collect();

    Ok(accounts)
}

/// Reconcile open positions and orders after a crash or restart.
async fn do_reconcile(inner: Arc<CtraderInner>) -> Result<ReconcileResult, BrokerError> {
    let ctid = inner
        .ctid_account_id
        .lock()
        .unwrap()
        .ok_or_else(|| BrokerError::AuthFailed("Not authenticated".into()))?;

    let rec_req = ProtoOaReconcileReq {
        payload_type: None,
        ctid_trader_account_id: ctid,
        return_protection_orders: Some(false),
    };
    let res: ProtoOaReconcileRes = request(&inner, PT_RECONCILE_REQ, &rec_req, "reconcile").await?;

    let open_position_ids: Vec<u64> = res.position.iter().map(|p| p.position_id as u64).collect();

    let result = ReconcileResult {
        open_position_count: res.position.len(),
        pending_order_count: res.order.len(),
        open_position_ids,
    };

    info!(
        "Reconcile: {} open positions, {} pending orders",
        result.open_position_count, result.pending_order_count
    );
    Ok(result)
}

// ── Public CtraderClient ──────────────────────────────────────────────────────

/// cTrader TCP/TLS client.
///
/// Owns an internal `tokio::Runtime` so all async operations can be driven
/// from sync call-sites (including the `Broker` trait methods).
pub struct CtraderClient {
    config: CtraderConfig,
    inner: Arc<CtraderInner>,
    rt: tokio::runtime::Runtime,
}

impl CtraderClient {
    pub fn new(config: CtraderConfig) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("gadarah-ctrader")
            .worker_threads(2)
            .build()
            .expect("Failed to build tokio runtime for cTrader client");
        Self {
            config,
            inner: CtraderInner::new(),
            rt,
        }
    }

    /// Connect, authenticate, load symbols, subscribe to spots.
    /// Must be called before any `Broker` trait method.
    pub fn connect_blocking(&mut self) -> Result<(), BrokerError> {
        let inner = Arc::clone(&self.inner);
        let config = self.config.clone();
        self.rt.block_on(connect_and_auth(inner, config))
    }

    /// Tear down the current connection state so a fresh `connect_blocking`
    /// can establish a new TLS session.  Safe to call when already disconnected.
    fn reset_inner(&mut self) {
        self.inner.connected.store(false, Ordering::Release);
        self.inner.authenticated.store(false, Ordering::Release);
        // Drop the writer channel so the writer task exits.
        *self.inner.write_tx.lock().unwrap() = None;
        // Fail any in-flight requests.
        {
            let mut pending = self.inner.pending.lock().unwrap();
            for (_, tx) in pending.drain() {
                let _ = tx.send(Err(BrokerError::Connection("Reset for reconnect".into())));
            }
        }
        // Replace inner so background tasks (reader/heartbeat) that hold the
        // old Arc can drain without interfering with the new session.
        self.inner = CtraderInner::new();
    }

    /// Reconnect with exponential backoff.  Returns `Ok(())` once reconnected
    /// and re-authenticated, or `Err` if `max_attempts` is exhausted.
    pub fn reconnect_blocking(&mut self, max_attempts: u32) -> Result<(), BrokerError> {
        let mut delay = Duration::from_secs(1);
        let max_delay = Duration::from_secs(60);

        for attempt in 1..=max_attempts {
            warn!(
                "Reconnect attempt {}/{} (backoff {:?})",
                attempt, max_attempts, delay
            );
            self.reset_inner();
            std::thread::sleep(delay);

            match self.connect_blocking() {
                Ok(()) => {
                    info!("Reconnected on attempt {attempt}");
                    return Ok(());
                }
                Err(e) => {
                    error!("Reconnect attempt {attempt} failed: {e}");
                    delay = (delay * 2).min(max_delay);
                }
            }
        }
        Err(BrokerError::Connection(format!(
            "Failed to reconnect after {max_attempts} attempts"
        )))
    }

    /// Reconcile open positions/orders — call after a restart.
    pub fn reconcile_blocking(&mut self) -> Result<ReconcileResult, BrokerError> {
        let inner = Arc::clone(&self.inner);
        self.rt.block_on(do_reconcile(inner))
    }

    /// Connect (app-auth only) and list trading accounts for an access token.
    /// Used during the OAuth flow — does not require account credentials.
    pub fn list_accounts_blocking(
        &mut self,
        access_token: &str,
    ) -> Result<Vec<crate::auth::TradingAccount>, BrokerError> {
        let inner = Arc::clone(&self.inner);
        let config = self.config.clone();
        self.rt.block_on(async {
            connect_app_only(Arc::clone(&inner), config).await?;
            do_list_accounts(inner, access_token).await
        })
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Acquire)
    }

    pub fn is_authenticated(&self) -> bool {
        self.inner.authenticated.load(Ordering::Acquire)
    }

    // ── Internal async helpers ─────────────────────────────────────────────

    async fn send_order_async(&self, req: &OrderRequest) -> Result<FillReport, BrokerError> {
        let ctid = self
            .inner
            .ctid_account_id
            .lock()
            .unwrap()
            .ok_or_else(|| BrokerError::AuthFailed("Not authenticated".into()))?;

        let symbol_id = self
            .inner
            .symbols
            .lock()
            .unwrap()
            .get(&req.symbol)
            .copied()
            .ok_or_else(|| BrokerError::InvalidSymbol(req.symbol.clone()))?;

        let trade_side = match req.direction {
            gadarah_core::Direction::Buy => ProtoOaTradeSide::Buy as i32,
            gadarah_core::Direction::Sell => ProtoOaTradeSide::Sell as i32,
        };

        let (oa_order_type, limit_price, stop_price) = match req.order_type {
            OrderType::Market => (ProtoOaOrderType::Market as i32, None, None),
            OrderType::Limit { price } => (
                ProtoOaOrderType::Limit as i32,
                Some(price.to_string().parse::<f64>().unwrap_or(0.0)),
                None,
            ),
            OrderType::Stop { price } => (
                ProtoOaOrderType::Stop as i32,
                None,
                Some(price.to_string().parse::<f64>().unwrap_or(0.0)),
            ),
        };

        let sl = req
            .stop_loss
            .is_sign_positive()
            .then(|| req.stop_loss.to_string().parse::<f64>().unwrap_or(0.0));
        let tp = req
            .take_profit
            .is_sign_positive()
            .then(|| req.take_profit.to_string().parse::<f64>().unwrap_or(0.0));

        let order_req = ProtoOaNewOrderReq {
            payload_type: None,
            ctid_trader_account_id: ctid,
            symbol_id,
            order_type: oa_order_type,
            trade_side,
            volume: lots_to_volume(&req.lots),
            limit_price,
            stop_price,
            stop_loss: sl,
            take_profit: tp,
            label: Some("GADARAH".to_string()),
            comment: Some(req.comment.clone()),
            ..Default::default()
        };

        let msg_id = self.inner.next_msg_id();
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(msg_id.clone(), tx);

        let frame = encode_message(PT_NEW_ORDER_REQ, &order_req, Some(msg_id))
            .map_err(|e| BrokerError::Protocol(e.to_string()))?;
        self.inner.queue_frame(frame.freeze())?;

        let payload = tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| BrokerError::Timeout {
                operation: "order placement".to_string(),
            })?
            .map_err(|_| BrokerError::Connection("Response channel dropped".into()))??;

        let event = ProtoOaExecutionEvent::decode(payload.as_ref())
            .map_err(|e| BrokerError::Protocol(e.to_string()))?;

        let deal = event
            .deal
            .as_ref()
            .ok_or_else(|| BrokerError::Protocol("ExecutionEvent missing deal".into()))?;

        let fill_price = deal
            .execution_price
            .map(|p| Decimal::try_from(p).unwrap_or(Decimal::ZERO))
            .unwrap_or(Decimal::ZERO);

        let position_id = event
            .position
            .as_ref()
            .map(|p| p.position_id as u64)
            .unwrap_or(0);

        let commission = deal
            .commission
            .map(cents_to_decimal)
            .unwrap_or(Decimal::ZERO)
            .abs();

        Ok(FillReport {
            position_id,
            fill_price,
            filled_lots: volume_to_lots(deal.filled_volume),
            fill_time: deal.execution_timestamp / 1000,
            slippage_pips: Decimal::ZERO,
            commission,
        })
    }

    async fn modify_position_async(&self, req: &ModifyRequest) -> Result<(), BrokerError> {
        let ctid = self
            .inner
            .ctid_account_id
            .lock()
            .unwrap()
            .ok_or_else(|| BrokerError::AuthFailed("Not authenticated".into()))?;

        let amend = ProtoOaAmendPositionSltpReq {
            payload_type: None,
            ctid_trader_account_id: ctid,
            position_id: req.position_id as i64,
            stop_loss: req.new_sl.and_then(|p| p.to_string().parse::<f64>().ok()),
            take_profit: req.new_tp.and_then(|p| p.to_string().parse::<f64>().ok()),
            ..Default::default()
        };

        let msg_id = self.inner.next_msg_id();
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(msg_id.clone(), tx);

        let frame = encode_message(PT_AMEND_SLTP_REQ, &amend, Some(msg_id))
            .map_err(|e| BrokerError::Protocol(e.to_string()))?;
        self.inner.queue_frame(frame.freeze())?;

        tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| BrokerError::Timeout {
                operation: "modify position".to_string(),
            })?
            .map_err(|_| BrokerError::Connection("Response channel dropped".into()))??;

        Ok(())
    }

    async fn close_position_async(&self, req: &CloseRequest) -> Result<CloseReport, BrokerError> {
        let ctid = self
            .inner
            .ctid_account_id
            .lock()
            .unwrap()
            .ok_or_else(|| BrokerError::AuthFailed("Not authenticated".into()))?;

        // Volume: None means close-all. We need the current position volume for that.
        // For simplicity, if None is passed we use a sentinel large value; cTrader
        // will close the full position.
        let volume = req.lots.as_ref().map(lots_to_volume).unwrap_or(i64::MAX); // cTrader accepts i64::MAX as "close all"

        let close_req = ProtoOaClosePositionReq {
            payload_type: None,
            ctid_trader_account_id: ctid,
            position_id: req.position_id as i64,
            volume,
        };

        let msg_id = self.inner.next_msg_id();
        let (tx, rx) = oneshot::channel();
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(msg_id.clone(), tx);

        let frame = encode_message(PT_CLOSE_POSITION_REQ, &close_req, Some(msg_id))
            .map_err(|e| BrokerError::Protocol(e.to_string()))?;
        self.inner.queue_frame(frame.freeze())?;

        let payload = tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| BrokerError::Timeout {
                operation: "close position".to_string(),
            })?
            .map_err(|_| BrokerError::Connection("Response channel dropped".into()))??;

        let event = ProtoOaExecutionEvent::decode(payload.as_ref())
            .map_err(|e| BrokerError::Protocol(e.to_string()))?;

        let deal = event
            .deal
            .as_ref()
            .ok_or_else(|| BrokerError::Protocol("Close ExecutionEvent missing deal".into()))?;

        let close_price = deal
            .execution_price
            .map(|p| Decimal::try_from(p).unwrap_or(Decimal::ZERO))
            .unwrap_or(Decimal::ZERO);

        Ok(CloseReport {
            position_id: req.position_id,
            close_price,
            closed_lots: volume_to_lots(deal.filled_volume),
            pnl: Decimal::ZERO, // gross P&L available in deal.gross_profit if needed
            close_time: deal.execution_timestamp / 1000,
            slippage_pips: Decimal::ZERO,
            commission: deal
                .commission
                .map(|c| cents_to_decimal(c).abs())
                .unwrap_or(Decimal::ZERO),
        })
    }
}

// ── Broker trait ──────────────────────────────────────────────────────────────

impl Broker for CtraderClient {
    fn send_order(&mut self, req: &OrderRequest) -> Result<FillReport, BrokerError> {
        let inner = Arc::clone(&self.inner);
        let _ = inner; // ensure inner lives; actual reference is via self below
        self.rt.block_on(self.send_order_async(req))
    }

    fn modify_position(&mut self, req: &ModifyRequest) -> Result<(), BrokerError> {
        self.rt.block_on(self.modify_position_async(req))
    }

    fn close_position(&mut self, req: &CloseRequest) -> Result<CloseReport, BrokerError> {
        self.rt.block_on(self.close_position_async(req))
    }

    fn get_tick(&self, symbol: &str) -> Result<Tick, BrokerError> {
        self.inner
            .ticks
            .lock()
            .unwrap()
            .get(symbol)
            .cloned()
            .ok_or_else(|| BrokerError::NoData(format!("No tick for {symbol}")))
    }

    fn get_spread_pips(&self, symbol: &str) -> Result<Decimal, BrokerError> {
        let tick = self.get_tick(symbol)?;
        // pip_size is symbol-dependent; use hardcoded defaults for common pairs
        let pip_size = pip_size_for(symbol);
        Ok(tick.spread_pips(pip_size))
    }

    fn account_info(&self) -> Result<BrokerAccountInfo, BrokerError> {
        self.inner
            .account_info
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| BrokerError::NoData("No account info yet".into()))
    }

    fn symbol_spec(&self, symbol: &str) -> Result<SymbolSpec, BrokerError> {
        let broker_symbol_id = self
            .inner
            .symbols
            .lock()
            .unwrap()
            .get(symbol)
            .copied()
            .unwrap_or(0);

        symbol_spec_for(symbol, broker_symbol_id)
    }

    fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Acquire)
    }
}

// ── Symbol spec helpers ───────────────────────────────────────────────────────

fn pip_size_for(symbol: &str) -> Decimal {
    match symbol {
        "USDJPY" | "XAUUSD" => Decimal::from_str_exact("0.01").unwrap(),
        _ => Decimal::from_str_exact("0.0001").unwrap(),
    }
}

fn symbol_spec_for(symbol: &str, broker_symbol_id: i64) -> Result<SymbolSpec, BrokerError> {
    let (pip_size, pip_value_per_lot, lot_size, typical_spread) = match symbol {
        "EURUSD" | "GBPUSD" | "USDCAD" | "AUDUSD" => (
            Decimal::from_str_exact("0.0001").unwrap(),
            Decimal::from(10),
            Decimal::from(100_000),
            Decimal::from_str_exact("0.5").unwrap(),
        ),
        "USDJPY" => (
            Decimal::from_str_exact("0.01").unwrap(),
            Decimal::from_str_exact("9.5").unwrap(), // approx, depends on rate
            Decimal::from(100_000),
            Decimal::from_str_exact("0.5").unwrap(),
        ),
        "XAUUSD" => (
            Decimal::from_str_exact("0.01").unwrap(),
            Decimal::from(1),
            Decimal::from(100),
            Decimal::from(20),
        ),
        _ => return Err(BrokerError::InvalidSymbol(symbol.to_string())),
    };

    Ok(SymbolSpec {
        name: symbol.to_string(),
        broker_symbol_id,
        pip_size,
        lot_size,
        pip_value_per_lot,
        min_volume: Decimal::from_str_exact("0.01").unwrap(),
        max_volume: Decimal::from(100),
        volume_step: Decimal::from_str_exact("0.01").unwrap(),
        swap_long: Decimal::ZERO,
        swap_short: Decimal::ZERO,
        typical_spread_pips: typical_spread,
        commission_per_lot: Decimal::from(5),
    })
}

// ── Backward-compat factory function ─────────────────────────────────────────

/// Create a `CtraderClient` (demo by default).
pub fn create_ctrader_broker(config: CtraderConfig) -> CtraderClient {
    CtraderClient::new(config)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = CtraderConfig::new("test_client_id".to_string(), "test_secret".to_string());
        assert_eq!(config.client_id, "test_client_id");
        assert!(config.access_token.is_none());
        assert!(config.is_demo, "should default to demo");
    }

    #[test]
    fn test_config_with_account() {
        let config = CtraderConfig::new("id".to_string(), "secret".to_string())
            .with_account("token123".to_string(), 12345);
        assert_eq!(config.access_token.as_deref(), Some("token123"));
        assert_eq!(config.ctid_account_id, Some(12345));
    }

    #[test]
    fn test_config_live_flag() {
        let config = CtraderConfig::new("id".to_string(), "s".to_string()).live();
        assert!(!config.is_demo);
        assert_eq!(config.host(), LIVE_HOST);
    }

    #[test]
    fn test_lots_volume_roundtrip() {
        let lots = Decimal::from_str_exact("0.10").unwrap();
        let volume = lots_to_volume(&lots);
        assert_eq!(volume, 10); // 0.10 lots * 100 = 10
        assert_eq!(volume_to_lots(volume), lots);
    }

    #[test]
    fn test_scaled_price() {
        // 1.23456 in cTrader = 123456 raw
        let raw: u64 = 123456;
        let dec = scaled_to_decimal(raw);
        assert_eq!(dec, Decimal::from_str_exact("1.23456").unwrap());
    }

    #[test]
    fn test_not_connected_returns_error() {
        let config = CtraderConfig::new("id".to_string(), "s".to_string());
        let client = CtraderClient::new(config);
        assert!(!client.is_connected());
        assert!(!client.is_authenticated());
        assert!(client.get_tick("EURUSD").is_err());
        assert!(client.account_info().is_err());
    }
}
