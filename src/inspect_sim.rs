//! Simulator (std) transport for the `debug-inspect` WebSocket debug server.
//!
//! Mirrors `inspect_esp.rs`'s structure (same message dispatch, same shared
//! `inspect_shared` logic) but with blocking `std::net`/`std::thread` instead
//! of `embassy_net`/`embassy_executor`, since the simulator has no async
//! runtime at all. No mDNS, no QR splash — see module docs on those in the
//! plan (`/Users/josh/.claude/plans/delegated-nibbling-panda.md`): a local
//! process on `localhost` doesn't need LAN discovery, and the simulator
//! already has a visible window, so it just logs the URL once.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use embedded_inspect::{
    CommandArg, CommandOutput, DebugCommands, DebugSetValue, Inspect, SetValueResult,
};
use embedded_websocket::{
    WebSocketCloseStatusCode, WebSocketReceiveMessageType, WebSocketSendMessageType,
    WebSocketServer,
};

use crate::hardware::HardwareAccess;
use crate::inspect_shared::{
    apply_mailbox, changed_resp, collect_leaf_paths, command_result_json, commands_response_json,
    debug_value_to_f64, debug_value_to_json, error_resp, hello_ack, inspect_state_schema_json,
    metric_batch_json, parse_command_args, parse_msg, parse_set_value, parse_string_array,
    schema_resp, screenshot_begin_resp, screenshot_end_resp, screenshot_unavailable_resp,
    set_value_ack, set_value_error, subscribe_metrics_ack_json, sync_from_real_state,
    unsubscribe_metrics_ack_json, value_resp, wrapping_checksum, InMsg, InspectState,
    MailboxEffects,
};

const PORT: u16 = 3000;
const DEVICE_NAME: &str = "ereader-sim";
const DEVICE_TYPE_STR: &str = "simulator";
const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
const SCREENSHOT_CHUNK_SIZE: usize = 4096;

const INDEX_HTML: &[u8] = include_bytes!("../assets/inspect_index.html");

// ── Command / set-value dispatch ────────────────────────────────────────────
//
// Unlike the ESP transport (one global `Channel`/`Signal` pair, since
// embassy's `Signal` can only hold one pending reply at a time), each request
// here carries its own one-shot `mpsc::Sender` for the reply — std makes this
// cheap and it sidesteps that limitation entirely.

pub struct CommandRequest {
    pub name: String,
    pub args: Vec<CommandArg>,
    pub reply: mpsc::Sender<CommandOutput>,
}
pub struct SetValueRequest {
    pub path: String,
    pub value: CommandArg,
    pub reply: mpsc::Sender<SetValueResult>,
}

pub struct FramebufferSnapshot {
    data: Vec<u8>,
    width: u16,
    height: u16,
    capture_id: u32,
}
pub type FbState = Arc<Mutex<Option<FramebufferSnapshot>>>;
static NEXT_CAPTURE_ID: AtomicU32 = AtomicU32::new(0);

/// Copies the current window framebuffer (RGB565, little-endian, native
/// `SimulatorDisplay` pixel format) for later streaming via `GetScreenshot`.
/// Call from the sim's SDL redraw path right after drawing, before
/// `window.update(&display)`.
pub fn capture_framebuffer(fb_state: &FbState, data: Vec<u8>, width: u16, height: u16) {
    let capture_id = NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed) + 1;
    *fb_state.lock().unwrap() = Some(FramebufferSnapshot { data, width, height, capture_id });
}

/// Owns everything the `'sim_running` loop needs to keep the debug server
/// fed: the shared `InspectState`, the framebuffer slot, and the two command
/// channels. Construct once via `spawn()`; call `tick()` once per loop
/// iteration (it throttles itself — see `tick`'s doc).
pub struct SimServer {
    pub inspect_state: Arc<Mutex<InspectState>>,
    pub fb_state: FbState,
    cmd_rx: mpsc::Receiver<CommandRequest>,
    set_rx: mpsc::Receiver<SetValueRequest>,
    boot: Instant,
    last_sync: Instant,
}

impl SimServer {
    /// Keeps `InspectState` current and drains pending commands/writes —
    /// throttled to once per 200ms since the simulator's SDL loop is an
    /// uncapped busy-poll with no inherent frame delay, so calling this
    /// unconditionally every spin would take the lock far more often than
    /// useful. Returns `None` on ticks where it did nothing (too soon).
    ///
    /// `orientation_changed` on the returned `MailboxEffects` needs the same
    /// handling as the sim's own `ORIENTATION_ID` click handler (recreating
    /// the SDL window/display at the new logical size) — that can only be
    /// done by `fn main()`, which owns those locals, not here.
    pub fn tick(&mut self, app: &mut crate::appstate::AppState, hw: &mut dyn HardwareAccess) -> Option<MailboxEffects> {
        if self.last_sync.elapsed() < Duration::from_millis(200) {
            return None;
        }
        self.last_sync = Instant::now();

        let uptime_secs = self.boot.elapsed().as_secs() as u32;
        {
            let mut s = self.inspect_state.lock().unwrap();
            sync_from_real_state(&mut s, app, hw, uptime_secs);
            // A local process on `localhost` has no real network state worth
            // reflecting — report the debug server's own loopback address.
            s.network.connected = true;
            s.network.ip_a = 127;
            s.network.ip_b = 0;
            s.network.ip_c = 0;
            s.network.ip_d = 1;
        }

        while let Ok(req) = self.cmd_rx.try_recv() {
            let output = self
                .inspect_state
                .lock()
                .unwrap()
                .dispatch_command(&req.name, &req.args)
                .unwrap_or_else(|e| CommandOutput::Error(format!("{:?}", e)));
            req.reply.send(output).ok();
        }
        while let Ok(req) = self.set_rx.try_recv() {
            let result = self.inspect_state.lock().unwrap().set_field(&req.path, req.value);
            req.reply.send(result).ok();
        }

        let effects = {
            let mut s = self.inspect_state.lock().unwrap();
            apply_mailbox(&mut s, app, hw)
        };
        if effects.ntp_sync_requested {
            log::info!("debug-inspect (sim): NTP sync requested, but the simulator has no network sync capability");
        }
        Some(effects)
    }
}

/// Starts the debug server on a background OS thread and returns the handle
/// `fn main()` uses to keep it fed. Call once, near the top of `fn main()`.
pub fn spawn() -> SimServer {
    let inspect_state = Arc::new(Mutex::new(InspectState::default()));
    let fb_state: FbState = Arc::new(Mutex::new(None));
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let (set_tx, set_rx) = mpsc::channel();

    let state_for_thread = inspect_state.clone();
    let fb_for_thread = fb_state.clone();
    std::thread::spawn(move || run_server(state_for_thread, fb_for_thread, cmd_tx, set_tx));

    SimServer {
        inspect_state,
        fb_state,
        cmd_rx,
        set_rx,
        boot: Instant::now(),
        last_sync: Instant::now() - Duration::from_secs(1), // sync immediately on first tick
    }
}

fn run_server(
    state: Arc<Mutex<InspectState>>,
    fb_state: FbState,
    cmd_tx: mpsc::Sender<CommandRequest>,
    set_tx: mpsc::Sender<SetValueRequest>,
) {
    let listener = match TcpListener::bind(("0.0.0.0", PORT)) {
        Ok(l) => l,
        Err(e) => {
            log::warn!("debug-inspect (sim): failed to bind port {}: {}", PORT, e);
            return;
        }
    };
    log::info!("debug-inspect (sim): listening at ws://127.0.0.1:{}/", PORT);
    log::info!("debug-inspect (sim): dev console at http://127.0.0.1:{}/", PORT);

    // One connection at a time, mirroring the ESP transport's accept loop —
    // a local debug tool doesn't need concurrent browser tabs.
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        log::info!("debug-inspect (sim): client connected");
        handle_connection(stream, &state, &fb_state, &cmd_tx, &set_tx);
        log::info!("debug-inspect (sim): client disconnected");
    }
}

fn handle_connection(
    mut stream: TcpStream,
    state: &Arc<Mutex<InspectState>>,
    fb_state: &FbState,
    cmd_tx: &mpsc::Sender<CommandRequest>,
    set_tx: &mpsc::Sender<SetValueRequest>,
) {
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();

    let mut http_buf = vec![0u8; 1536];
    let mut http_len = 0usize;
    loop {
        match stream.read(&mut http_buf[http_len..]) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                http_len += n;
                if http_buf[..http_len].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if http_len >= http_buf.len() {
                    return;
                }
            }
        }
    }

    let mut headers = [httparse::EMPTY_HEADER; 24];
    let mut request = httparse::Request::new(&mut headers);
    if request.parse(&http_buf[..http_len]).is_err() {
        return;
    }

    match embedded_websocket::read_http_header(request.headers.iter().map(|h| (h.name, h.value))) {
        Ok(Some(ctx)) => {
            let mut ws = WebSocketServer::new_server();
            let mut resp_buf = vec![0u8; 512];
            let n = match ws.server_accept(&ctx.sec_websocket_key, None, &mut resp_buf) {
                Ok(n) => n,
                Err(_) => return,
            };
            if stream.write_all(&resp_buf[..n]).is_err() {
                return;
            }
            log::debug!("debug-inspect (sim): WebSocket upgrade OK");
            run_ws_session(&mut ws, &mut stream, state, fb_state, cmd_tx, set_tx);
        }
        _ => {
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                INDEX_HTML.len()
            );
            stream.write_all(header.as_bytes()).ok();
            stream.write_all(INDEX_HTML).ok();
        }
    }
}

fn handle_msg(json: &str, state: &Arc<Mutex<InspectState>>, schema_json: &str) -> String {
    match parse_msg(json) {
        InMsg::Hello { request_id } => {
            log::info!("debug-inspect (sim): Hello request_id={}", request_id);
            hello_ack(request_id, DEVICE_NAME, DEVICE_TYPE_STR, FIRMWARE_VERSION, "")
        }
        InMsg::GetSchema { request_id } => schema_resp(request_id, schema_json),
        InMsg::GetValue { request_id, path } => {
            let result = state.lock().unwrap().get_field_path(path).map(debug_value_to_json);
            match result {
                Some(v) => value_resp(request_id, path, &v),
                None => error_resp(request_id, "UnknownPath", &format!("no field at path: {}", path)),
            }
        }
        InMsg::GetCommands { request_id } => {
            commands_response_json(request_id, <InspectState as DebugCommands>::command_defs())
        }
        InMsg::GetScreenshot { .. }
        | InMsg::InvokeCommand { .. }
        | InMsg::SetValue { .. }
        | InMsg::SubscribeMetrics { .. }
        | InMsg::UnsubscribeMetrics { .. }
        | InMsg::Unknown => error_resp(0, "UnknownMessage", "unrecognised message type"),
    }
}

/// Streams the most recent framebuffer snapshot. Wire format tag is
/// `"RGB565LE"` (2 bytes/pixel, little-endian) — deliberately different from
/// the ESP transport's `"Gray4"`, since the two targets' framebuffers really
/// are different pixel formats; the browser-side viewer branches on the
/// `format` field in `ScreenshotBegin`.
fn send_screenshot(ws: &mut WebSocketServer, sock: &mut TcpStream, request_id: u32, tx_buf: &mut Vec<u8>, fb_state: &FbState) -> bool {
    let meta = fb_state
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| (s.capture_id, s.width, s.height, s.data.len() as u32));
    let (capture_id, width, height, total_bytes) = match meta {
        None => {
            let msg = screenshot_unavailable_resp(request_id);
            let n = ws.write(WebSocketSendMessageType::Text, true, msg.as_bytes(), tx_buf).unwrap_or(0);
            return sock.write_all(&tx_buf[..n]).is_ok();
        }
        Some(m) => m,
    };

    let total_chunks = ((total_bytes as usize + SCREENSHOT_CHUNK_SIZE - 1) / SCREENSHOT_CHUNK_SIZE) as u32;
    let begin = screenshot_begin_resp(capture_id, width, height, "RGB565LE", total_bytes, SCREENSHOT_CHUNK_SIZE as u16, total_chunks);
    let n = ws.write(WebSocketSendMessageType::Text, true, begin.as_bytes(), tx_buf).unwrap_or(0);
    if sock.write_all(&tx_buf[..n]).is_err() {
        return false;
    }

    let mut pixel_frame = vec![0u8; SCREENSHOT_CHUNK_SIZE + 8];
    let mut ws_frame = vec![0u8; SCREENSHOT_CHUNK_SIZE + 8 + 16];
    let mut total_checksum: u32 = 0;

    for chunk_index in 0..total_chunks {
        let offset = chunk_index as usize * SCREENSHOT_CHUNK_SIZE;
        let end = (offset + SCREENSHOT_CHUNK_SIZE).min(total_bytes as usize);
        let chunk_len = end - offset;

        pixel_frame[0..4].copy_from_slice(&capture_id.to_le_bytes());
        pixel_frame[4..8].copy_from_slice(&chunk_index.to_le_bytes());

        let ok = {
            let guard = fb_state.lock().unwrap();
            match guard.as_ref() {
                Some(snap) if snap.capture_id == capture_id => {
                    pixel_frame[8..8 + chunk_len].copy_from_slice(&snap.data[offset..end]);
                    true
                }
                _ => false,
            }
        };
        if !ok {
            log::warn!("debug-inspect (sim): screenshot snapshot replaced mid-transfer, aborting");
            return true;
        }

        total_checksum = total_checksum.wrapping_add(wrapping_checksum(&pixel_frame[8..8 + chunk_len]));

        let n = ws
            .write(WebSocketSendMessageType::Binary, true, &pixel_frame[..8 + chunk_len], &mut ws_frame)
            .unwrap_or(0);
        if sock.write_all(&ws_frame[..n]).is_err() {
            return false;
        }
    }

    let end_msg = screenshot_end_resp(capture_id, total_chunks, total_checksum);
    let n = ws.write(WebSocketSendMessageType::Text, true, end_msg.as_bytes(), tx_buf).unwrap_or(0);
    sock.write_all(&tx_buf[..n]).is_ok()
}

fn run_ws_session(
    ws: &mut WebSocketServer,
    sock: &mut TcpStream,
    state: &Arc<Mutex<InspectState>>,
    fb_state: &FbState,
    cmd_tx: &mpsc::Sender<CommandRequest>,
    set_tx: &mpsc::Sender<SetValueRequest>,
) {
    let schema_json = inspect_state_schema_json();
    let default_state = InspectState::default();
    let mut leaf_paths: Vec<String> = Vec::new();
    collect_leaf_paths(&default_state, "", &mut leaf_paths);
    let mut snapshot: Vec<String> = leaf_paths.iter().map(|_| String::new()).collect();
    let mut seq: u32 = 0;

    let mut rx_buf = vec![0u8; 2048];
    let mut pl_buf = vec![0u8; 1536];
    let mut tx_buf = vec![0u8; 2048];
    let mut buf_used = 0usize;

    let mut last_event = Instant::now();
    let mut subscribed_metric_paths: Vec<String> = Vec::new();
    let mut metric_interval_ms: u32 = 0;
    let mut last_metric: Option<Instant> = None;

    'outer: loop {
        if !subscribed_metric_paths.is_empty() && metric_interval_ms > 0 {
            let should_sample = match last_metric {
                None => true,
                Some(t) => t.elapsed() >= Duration::from_millis(metric_interval_ms as u64),
            };
            if should_sample {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let snap: InspectState = state.lock().unwrap().clone();
                let samples: Vec<(String, f64)> = subscribed_metric_paths
                    .iter()
                    .filter_map(|path| snap.get_field_path(path).and_then(debug_value_to_f64).map(|v| (path.clone(), v)))
                    .collect();
                if !samples.is_empty() {
                    let json = metric_batch_json(ts, metric_interval_ms, &samples);
                    let n = ws.write(WebSocketSendMessageType::Text, true, json.as_bytes(), &mut tx_buf).unwrap_or(0);
                    if n > 0 && sock.write_all(&tx_buf[..n]).is_err() {
                        break 'outer;
                    }
                }
                last_metric = Some(Instant::now());
            }
        }

        if last_event.elapsed() >= Duration::from_secs(2) {
            let snap: InspectState = state.lock().unwrap().clone();
            for (i, path) in leaf_paths.iter().enumerate() {
                if let Some(val) = snap.get_field_path(path).map(debug_value_to_json) {
                    if snapshot[i] != val {
                        snapshot[i] = val.clone();
                        let msg = changed_resp(path, &val, seq);
                        seq += 1;
                        let n = ws.write(WebSocketSendMessageType::Text, true, msg.as_bytes(), &mut tx_buf).unwrap_or(0);
                        if n > 0 && sock.write_all(&tx_buf[..n]).is_err() {
                            break 'outer;
                        }
                    }
                }
            }
            last_event = Instant::now();
        }

        // Blocking read with a short timeout — acts as the "poll" step; a
        // WouldBlock/TimedOut just means "nothing arrived yet, loop again
        // and re-check the push conditions above" (the sync equivalent of
        // the ESP transport's `with_timeout(...).await`).
        stream_set_short_timeout(sock);
        match sock.read(&mut rx_buf[buf_used..]) {
            Ok(0) => break,
            Ok(n) => buf_used += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => break,
        }

        'inner: loop {
            if buf_used == 0 {
                break 'inner;
            }
            match ws.read(&rx_buf[..buf_used], &mut pl_buf) {
                Err(_) => break 'outer,
                Ok(r) if r.len_from == 0 => break 'inner,
                Ok(r) => {
                    let consumed = r.len_from;
                    rx_buf.copy_within(consumed..buf_used, 0);
                    buf_used -= consumed;
                    let payload = &pl_buf[..r.len_to];

                    match r.message_type {
                        WebSocketReceiveMessageType::Text => {
                            let json = match core::str::from_utf8(payload) {
                                Ok(j) => j,
                                Err(_) => {
                                    let msg = error_resp(0, "BadEncoding", "non-UTF-8");
                                    let n = ws.write(WebSocketSendMessageType::Text, true, msg.as_bytes(), &mut tx_buf).unwrap_or(0);
                                    if sock.write_all(&tx_buf[..n]).is_err() {
                                        break 'outer;
                                    }
                                    break 'inner;
                                }
                            };
                            match parse_msg(json) {
                                InMsg::GetScreenshot { request_id } => {
                                    if !send_screenshot(ws, sock, request_id, &mut tx_buf, fb_state) {
                                        break 'outer;
                                    }
                                    break 'inner;
                                }
                                InMsg::InvokeCommand { request_id, name, args_json } => {
                                    let args = parse_command_args(args_json);
                                    let (reply_tx, reply_rx) = mpsc::channel();
                                    cmd_tx
                                        .send(CommandRequest { name: String::from(name), args, reply: reply_tx })
                                        .ok();
                                    let reply = match reply_rx.recv_timeout(Duration::from_secs(5)) {
                                        Ok(output) => command_result_json(request_id, &output, 0),
                                        Err(_) => command_result_json(
                                            request_id,
                                            &CommandOutput::Error(String::from("timeout: device busy")),
                                            0,
                                        ),
                                    };
                                    let n = ws.write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf).unwrap_or(0);
                                    if sock.write_all(&tx_buf[..n]).is_err() {
                                        break 'outer;
                                    }
                                }
                                InMsg::SetValue { request_id, path, value_json } => {
                                    let reply = match parse_set_value(value_json) {
                                        None => set_value_error(request_id, "MalformedRequest", path),
                                        Some(value) => {
                                            let (reply_tx, reply_rx) = mpsc::channel();
                                            set_tx
                                                .send(SetValueRequest { path: String::from(path), value, reply: reply_tx })
                                                .ok();
                                            match reply_rx.recv_timeout(Duration::from_secs(5)) {
                                                Ok(SetValueResult::Ok) => set_value_ack(request_id, path),
                                                Ok(SetValueResult::ReadOnly) => set_value_error(request_id, "ReadOnly", path),
                                                Ok(SetValueResult::TypeMismatch) => set_value_error(request_id, "TypeMismatch", path),
                                                Ok(SetValueResult::OutOfBounds) => set_value_error(request_id, "OutOfBounds", path),
                                                Ok(SetValueResult::UnknownField) => set_value_error(request_id, "UnknownPath", path),
                                                Ok(SetValueResult::UnknownVariant) => set_value_error(request_id, "UnknownVariant", path),
                                                Err(_) => set_value_error(request_id, "Timeout", path),
                                            }
                                        }
                                    };
                                    let n = ws.write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf).unwrap_or(0);
                                    if sock.write_all(&tx_buf[..n]).is_err() {
                                        break 'outer;
                                    }
                                }
                                InMsg::SubscribeMetrics { request_id, paths_json, interval_ms } => {
                                    let paths = parse_string_array(paths_json);
                                    let effective_interval = interval_ms.max(100);
                                    for p in &paths {
                                        if !subscribed_metric_paths.contains(p) {
                                            subscribed_metric_paths.push(p.clone());
                                        }
                                    }
                                    metric_interval_ms = effective_interval;
                                    last_metric = None;
                                    let reply = subscribe_metrics_ack_json(request_id, effective_interval, &subscribed_metric_paths);
                                    let n = ws.write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf).unwrap_or(0);
                                    if sock.write_all(&tx_buf[..n]).is_err() {
                                        break 'outer;
                                    }
                                }
                                InMsg::UnsubscribeMetrics { request_id, paths_json } => {
                                    let paths = parse_string_array(paths_json);
                                    if paths.is_empty() {
                                        subscribed_metric_paths.clear();
                                        metric_interval_ms = 0;
                                        last_metric = None;
                                    } else {
                                        subscribed_metric_paths.retain(|p| !paths.contains(p));
                                    }
                                    let reply = unsubscribe_metrics_ack_json(request_id, &subscribed_metric_paths);
                                    let n = ws.write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf).unwrap_or(0);
                                    if sock.write_all(&tx_buf[..n]).is_err() {
                                        break 'outer;
                                    }
                                }
                                _ => {
                                    let reply = handle_msg(json, state, &schema_json);
                                    let n = ws.write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf).unwrap_or(0);
                                    if sock.write_all(&tx_buf[..n]).is_err() {
                                        break 'outer;
                                    }
                                }
                            }
                        }
                        WebSocketReceiveMessageType::CloseMustReply => {
                            let n = ws.close(WebSocketCloseStatusCode::NormalClosure, None, &mut tx_buf).unwrap_or(0);
                            sock.write_all(&tx_buf[..n]).ok();
                            break 'outer;
                        }
                        WebSocketReceiveMessageType::Ping => {
                            let n = ws.write(WebSocketSendMessageType::Pong, true, payload, &mut tx_buf).unwrap_or(0);
                            if sock.write_all(&tx_buf[..n]).is_err() {
                                break 'outer;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn stream_set_short_timeout(sock: &mut TcpStream) {
    sock.set_read_timeout(Some(Duration::from_millis(20))).ok();
}
