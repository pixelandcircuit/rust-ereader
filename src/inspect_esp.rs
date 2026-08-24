//! ESP32-S3/embassy transport for the `debug-inspect` WebSocket debug server.
//!
//! Filled in incrementally: WiFi always-on task (stage 2), TCP/WebSocket
//! server (stage 3, this file so far), command/set-value dispatch (stage 4),
//! screenshot capture + mDNS + on-screen QR (stage 5). See
//! `/Users/josh/.claude/plans/delegated-nibbling-panda.md` for the full plan.
//!
//! Ported from the proven reference implementation at
//! `../../epaper-examples/examples/inspect_demo.rs`.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, IpEndpoint, Stack};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use embedded_websocket::{
    WebSocketCloseStatusCode, WebSocketReceiveMessageType, WebSocketSendMessageType,
    WebSocketServer,
};
use esp_radio::wifi::WifiController;

use crate::inspect_shared::{
    changed_resp, collect_leaf_paths, command_result_json, commands_response_json,
    debug_value_to_f64, debug_value_to_json, error_resp, hello_ack, inspect_state_schema_json,
    metric_batch_json, parse_command_args, parse_msg, parse_set_value, parse_string_array,
    schema_resp, screenshot_begin_resp, screenshot_end_resp, screenshot_unavailable_resp,
    set_value_ack, set_value_error, slugify, subscribe_metrics_ack_json,
    unsubscribe_metrics_ack_json, value_resp, wrapping_checksum, InMsg, InspectState,
};
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embedded_inspect::{CommandArg, CommandOutput, DebugCommands, DebugSetValue, Inspect, SetValueResult};

const PORT: u16 = 3000;
const DEVICE_NAME: &str = "ereader";
const DEVICE_TYPE_STR: &str = "ESP32-S3";
const FIRMWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
const SCREENSHOT_CHUNK_SIZE: usize = 4096;

const INDEX_HTML: &[u8] = include_bytes!("../assets/inspect_index.html");

pub type SharedInspectState = Mutex<CriticalSectionRawMutex, RefCell<InspectState>>;

// ── Command / set-value dispatch ────────────────────────────────────────────
//
// The debug server task can't hold `&mut AppState`/`&mut dyn HardwareAccess`
// (the `ui_task` owns those) — so `run_ws_session` sends a request on these
// channels and awaits the matching `Signal`, and `apply_pending_commands`
// (called from `ui_task` each tick) drains the channel, applies the command/
// write to `InspectState`, and signals the result back. Same request/response
// pattern already used by this codebase for `BOOK_LOAD_REQUEST`/`_RESULT`.

pub struct CommandRequest {
    pub request_id: u32,
    pub name: String,
    pub args: Vec<CommandArg>,
}
pub struct CommandResponse {
    pub request_id: u32,
    pub output: CommandOutput,
    pub duration_ms: u32,
}
pub struct SetValueRequest {
    pub request_id: u32,
    pub path: String,
    pub value: CommandArg,
}

static CMD_CHANNEL: Channel<CriticalSectionRawMutex, CommandRequest, 4> = Channel::new();
static CMD_RESP: Signal<CriticalSectionRawMutex, CommandResponse> = Signal::new();
static SET_CHANNEL: Channel<CriticalSectionRawMutex, SetValueRequest, 4> = Channel::new();
static SET_RESP: Signal<CriticalSectionRawMutex, (u32, SetValueResult)> = Signal::new();

/// Drains `CMD_CHANNEL`/`SET_CHANNEL`, applies commands/writes to
/// `InspectState`, signals results back to the waiting WS session, then
/// applies any mailbox fields left behind via the shared
/// `inspect_shared::apply_mailbox` (see there for what `MailboxEffects`
/// means and why it exists). Called from `ui_task` once per tick, right
/// after `sync_from_real_state`.
pub fn apply_pending_commands(
    state: &'static SharedInspectState,
    app: &mut crate::appstate::AppState,
    hw: &mut dyn crate::hardware::HardwareAccess,
) -> crate::inspect_shared::MailboxEffects {
    while let Ok(req) = CMD_CHANNEL.try_receive() {
        let output = state.lock(|cell| {
            cell.borrow_mut()
                .dispatch_command(&req.name, &req.args)
                .unwrap_or_else(|e| CommandOutput::Error(alloc::format!("{:?}", e)))
        });
        CMD_RESP.signal(CommandResponse {
            request_id: req.request_id,
            output,
            duration_ms: 0,
        });
    }
    while let Ok(req) = SET_CHANNEL.try_receive() {
        let result = state.lock(|cell| cell.borrow_mut().set_field(&req.path, req.value));
        SET_RESP.signal((req.request_id, result));
    }

    state.lock(|cell| crate::inspect_shared::apply_mailbox(&mut cell.borrow_mut(), app, hw))
}

/// Keeps `InspectState.network` current — split out from `sync_from_real_state`
/// because network status comes from the `embassy_net::Stack`, which `ui_task`
/// doesn't otherwise need (avoiding a `stack` parameter on a task that's
/// spawned before the network stack exists in `main`'s current ordering).
#[embassy_executor::task]
pub async fn network_status_task(stack: Stack<'static>, state: &'static SharedInspectState) {
    loop {
        let connected = stack.is_link_up();
        let ip = stack.config_v4().map(|c| c.address.address().octets());
        state.lock(|cell| {
            let mut s = cell.borrow_mut();
            s.network.connected = connected;
            if let Some(octets) = ip {
                s.network.ip_a = octets[0];
                s.network.ip_b = octets[1];
                s.network.ip_c = octets[2];
                s.network.ip_d = octets[3];
            }
        });
        Timer::after(Duration::from_secs(2)).await;
    }
}

/// Persistent WiFi reconnect loop — unlike the default build's `wifi_task`
/// (connect, sync NTP, disconnect), this holds the connection up indefinitely
/// so the debug server stays reachable. Only spawned when `debug-inspect` is
/// enabled; ported from the proven reference at
/// `../epaper-examples/examples/inspect_demo.rs:1490-1504`.
#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
    loop {
        match controller.connect_async().await {
            Ok(_) => {
                log::info!("debug-inspect: wifi connected");
                controller.wait_for_disconnect_async().await.ok();
                log::warn!("debug-inspect: wifi disconnected, retrying in 5s");
            }
            Err(e) => {
                log::warn!("debug-inspect: wifi connect failed: {:?}", e);
            }
        }
        Timer::after(Duration::from_secs(5)).await;
    }
}

// ── Screenshot capture ───────────────────────────────────────────────────────
//
// `Rgb565ToGray4`'s flush methods (in `examples/ereader_ui.rs`) call
// `capture_framebuffer` right before delegating to `Display::flush`/
// `flush_region`, since both reset the framebuffer to all-white afterward —
// that's the only valid capture window. `send_screenshot` streams the most
// recent snapshot to a WS client on `GetScreenshot`.

struct FramebufferSnapshot {
    data: Vec<u8>,
    capture_id: u32,
}
type FbState = Mutex<CriticalSectionRawMutex, RefCell<Option<FramebufferSnapshot>>>;
static FB_STATE: FbState = Mutex::new(RefCell::new(None));
static NEXT_CAPTURE_ID: AtomicU32 = AtomicU32::new(0);

/// Copies the current framebuffer for later streaming via `GetScreenshot`.
pub fn capture_framebuffer(display_bytes: &[u8]) {
    // Copy happens outside any lock — ~259 KB at 80 MB/s, interrupts stay
    // enabled; only the brief pointer swap below needs the critical section
    // (isolation rule 2).
    let data = Vec::from(display_bytes);
    let capture_id = NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed) + 1;
    FB_STATE.lock(|cell| {
        *cell.borrow_mut() = Some(FramebufferSnapshot { data, capture_id });
    });
}

/// Streams the most recent framebuffer snapshot as `ScreenshotBegin` +
/// binary chunks + `ScreenshotEnd`. Returns `false` on a socket write error
/// (caller should stop the session); a missing/replaced snapshot is reported
/// to the client instead, not treated as a transport failure.
async fn send_screenshot(
    ws: &mut WebSocketServer,
    sock: &mut TcpSocket<'_>,
    request_id: u32,
    tx_buf: &mut Vec<u8>,
) -> bool {
    let meta = FB_STATE.lock(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| (s.capture_id, s.data.len() as u32))
    });
    let (capture_id, total_bytes) = match meta {
        None => {
            let msg = screenshot_unavailable_resp(request_id);
            let n = ws
                .write(WebSocketSendMessageType::Text, true, msg.as_bytes(), tx_buf)
                .unwrap_or(0);
            return write_all(sock, &tx_buf[..n]).await.is_ok();
        }
        Some(m) => m,
    };

    let total_chunks = ((total_bytes as usize + SCREENSHOT_CHUNK_SIZE - 1) / SCREENSHOT_CHUNK_SIZE) as u32;
    let begin = screenshot_begin_resp(
        capture_id,
        crate::driver::display::DISPLAY_WIDTH,
        crate::driver::display::DISPLAY_HEIGHT,
        "Gray4",
        total_bytes,
        SCREENSHOT_CHUNK_SIZE as u16,
        total_chunks,
    );
    let n = ws
        .write(WebSocketSendMessageType::Text, true, begin.as_bytes(), tx_buf)
        .unwrap_or(0);
    if write_all(sock, &tx_buf[..n]).await.is_err() {
        return false;
    }

    // Binary frame payload: 8-byte header (capture_id LE + chunk_index LE) + pixel data.
    let mut pixel_frame = vec![0u8; SCREENSHOT_CHUNK_SIZE + 8];
    // WS-encoded output buffer (server frames are unmasked; framing overhead is small).
    let mut ws_frame = vec![0u8; SCREENSHOT_CHUNK_SIZE + 8 + 16];
    let mut total_checksum: u32 = 0;

    for chunk_index in 0..total_chunks {
        let offset = chunk_index as usize * SCREENSHOT_CHUNK_SIZE;
        let end = (offset + SCREENSHOT_CHUNK_SIZE).min(total_bytes as usize);
        let chunk_len = end - offset;

        pixel_frame[0..4].copy_from_slice(&capture_id.to_le_bytes());
        pixel_frame[4..8].copy_from_slice(&chunk_index.to_le_bytes());

        // Copy chunk data under a brief critical section (~50 us at 80 MB/s).
        let ok = FB_STATE.lock(|cell| {
            if let Some(snap) = cell.borrow().as_ref() {
                if snap.capture_id == capture_id {
                    pixel_frame[8..8 + chunk_len].copy_from_slice(&snap.data[offset..end]);
                    return true;
                }
            }
            false
        });
        if !ok {
            // Snapshot was replaced mid-transfer; the client will notice the
            // missing ScreenshotEnd and can retry.
            log::warn!("debug-inspect: screenshot snapshot replaced mid-transfer, aborting");
            return true;
        }

        total_checksum = total_checksum.wrapping_add(wrapping_checksum(&pixel_frame[8..8 + chunk_len]));

        let n = ws
            .write(WebSocketSendMessageType::Binary, true, &pixel_frame[..8 + chunk_len], &mut ws_frame)
            .unwrap_or(0);
        if write_all(sock, &ws_frame[..n]).await.is_err() {
            return false;
        }
    }

    let end_msg = screenshot_end_resp(capture_id, total_chunks, total_checksum);
    let n = ws
        .write(WebSocketSendMessageType::Text, true, end_msg.as_bytes(), tx_buf)
        .unwrap_or(0);
    write_all(sock, &tx_buf[..n]).await.is_ok()
}

// ── mDNS / DNS-SD ────────────────────────────────────────────────────────────
//
// Minimal mDNS implementation (RFC 6762 / RFC 6763): proactive announcements
// every 30 s, plus responses to A queries for `<hostname>.local` and PTR
// queries for `_ereader-inspect._tcp.local`. Ported from the proven reference
// at `../../epaper-examples/examples/inspect_demo.rs`.

const MDNS_IP: IpAddress = IpAddress::v4(224, 0, 0, 251);
const MDNS_SERVICE_TYPE: &str = "_ereader-inspect._tcp.local";

fn mdns_push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.push((v >> 8) as u8);
    buf.push(v as u8);
}
fn mdns_push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.push((v >> 24) as u8);
    buf.push((v >> 16) as u8);
    buf.push((v >> 8) as u8);
    buf.push(v as u8);
}
fn mdns_encode_name(buf: &mut Vec<u8>, name: &str) {
    for label in name.split('.') {
        if label.is_empty() {
            continue;
        }
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0); // root label
}
/// Encode a single DNS resource record (no name compression). class =
/// 0x8001 (IN | cache-flush bit for mDNS).
fn mdns_rr(buf: &mut Vec<u8>, name: &str, rtype: u16, ttl: u32, rdata: &[u8]) {
    mdns_encode_name(buf, name);
    mdns_push_u16(buf, rtype);
    mdns_push_u16(buf, 0x8001);
    mdns_push_u32(buf, ttl);
    mdns_push_u16(buf, rdata.len() as u16);
    buf.extend_from_slice(rdata);
}

/// Build a full mDNS announcement: PTR + SRV + TXT + A records.
fn build_mdns_announcement(hostname: &str, instance: &str, ip: [u8; 4]) -> Vec<u8> {
    let mut pkt = Vec::new();
    mdns_push_u16(&mut pkt, 0); // ID
    mdns_push_u16(&mut pkt, 0x8400); // Flags: QR=1, AA=1
    mdns_push_u16(&mut pkt, 0); // QDCOUNT
    mdns_push_u16(&mut pkt, 4); // ANCOUNT
    mdns_push_u16(&mut pkt, 0); // NSCOUNT
    mdns_push_u16(&mut pkt, 0); // ARCOUNT

    let full_instance = format!("{}.{}", instance, MDNS_SERVICE_TYPE);
    let hostname_local = format!("{}.local", hostname);

    {
        let mut rdata = Vec::new();
        mdns_encode_name(&mut rdata, &full_instance);
        mdns_rr(&mut pkt, MDNS_SERVICE_TYPE, 12, 4500, &rdata); // PTR
    }
    {
        let mut rdata = Vec::new();
        mdns_push_u16(&mut rdata, 0);
        mdns_push_u16(&mut rdata, 0);
        mdns_push_u16(&mut rdata, PORT);
        mdns_encode_name(&mut rdata, &hostname_local);
        mdns_rr(&mut pkt, &full_instance, 33, 120, &rdata); // SRV
    }
    {
        let mut rdata = Vec::new();
        for entry in &[
            format!("name={}", instance),
            format!("type={}", DEVICE_TYPE_STR),
            format!("version={}", FIRMWARE_VERSION),
            String::from("proto=1"),
        ] {
            rdata.push(entry.len() as u8);
            rdata.extend_from_slice(entry.as_bytes());
        }
        mdns_rr(&mut pkt, &full_instance, 16, 4500, &rdata); // TXT
    }
    mdns_rr(&mut pkt, &hostname_local, 1, 120, &ip); // A
    pkt
}

fn mdns_parse_name(pkt: &[u8], offset: &mut usize) -> Option<String> {
    let mut name = String::new();
    let mut pos = *offset;
    let mut jumped = false;
    let mut hops = 0usize;
    loop {
        if pos >= pkt.len() {
            return None;
        }
        let len = pkt[pos] as usize;
        if len & 0xC0 == 0xC0 {
            if pos + 1 >= pkt.len() {
                return None;
            }
            let ptr = ((len & 0x3F) << 8) | pkt[pos + 1] as usize;
            if !jumped {
                *offset = pos + 2;
            }
            pos = ptr;
            jumped = true;
            hops += 1;
            if hops > 16 {
                return None;
            }
            continue;
        }
        if len == 0 {
            if !jumped {
                *offset = pos + 1;
            }
            break;
        }
        pos += 1;
        if pos + len > pkt.len() {
            return None;
        }
        if !name.is_empty() {
            name.push('.');
        }
        name.push_str(core::str::from_utf8(&pkt[pos..pos + len]).ok()?);
        pos += len;
    }
    Some(name)
}

/// Parse an mDNS query and build a response if any questions match our records.
fn handle_mdns_query(pkt: &[u8], hostname: &str, instance: &str, ip: [u8; 4]) -> Option<Vec<u8>> {
    if pkt.len() < 12 {
        return None;
    }
    let flags = u16::from_be_bytes([pkt[2], pkt[3]]);
    if flags & 0x8000 != 0 {
        return None; // response, not a query
    }
    let qdcount = u16::from_be_bytes([pkt[4], pkt[5]]) as usize;
    if qdcount == 0 {
        return None;
    }

    let hostname_local = format!("{}.local", hostname);
    let mut want_a = false;
    let mut want_ptr = false;
    let mut offset = 12;
    for _ in 0..qdcount {
        let qname = mdns_parse_name(pkt, &mut offset)?;
        if offset + 4 > pkt.len() {
            return None;
        }
        let qtype = u16::from_be_bytes([pkt[offset], pkt[offset + 1]]);
        offset += 4;
        if qname.eq_ignore_ascii_case(&hostname_local) && (qtype == 1 || qtype == 255 || qtype == 28) {
            want_a = true;
        }
        if qname.eq_ignore_ascii_case(MDNS_SERVICE_TYPE) && (qtype == 12 || qtype == 255) {
            want_ptr = true;
        }
    }
    if !want_a && !want_ptr {
        return None;
    }

    let an_count = (want_ptr as u16) * 3 + (want_a as u16);
    let mut resp = Vec::new();
    let id = u16::from_be_bytes([pkt[0], pkt[1]]);
    mdns_push_u16(&mut resp, id);
    mdns_push_u16(&mut resp, 0x8400);
    mdns_push_u16(&mut resp, 0);
    mdns_push_u16(&mut resp, an_count);
    mdns_push_u16(&mut resp, 0);
    mdns_push_u16(&mut resp, 0);

    let full_instance = format!("{}.{}", instance, MDNS_SERVICE_TYPE);
    if want_ptr {
        let mut rdata = Vec::new();
        mdns_encode_name(&mut rdata, &full_instance);
        mdns_rr(&mut resp, MDNS_SERVICE_TYPE, 12, 4500, &rdata);

        let mut rdata = Vec::new();
        mdns_push_u16(&mut rdata, 0);
        mdns_push_u16(&mut rdata, 0);
        mdns_push_u16(&mut rdata, PORT);
        mdns_encode_name(&mut rdata, &hostname_local);
        mdns_rr(&mut resp, &full_instance, 33, 120, &rdata);

        let mut rdata = Vec::new();
        for entry in &[
            format!("name={}", instance),
            format!("type={}", DEVICE_TYPE_STR),
            format!("version={}", FIRMWARE_VERSION),
        ] {
            rdata.push(entry.len() as u8);
            rdata.extend_from_slice(entry.as_bytes());
        }
        mdns_rr(&mut resp, &full_instance, 16, 4500, &rdata);
    }
    if want_a {
        mdns_rr(&mut resp, &hostname_local, 1, 120, &ip);
    }
    Some(resp)
}

/// Announces this device on the LAN via mDNS/DNS-SD so
/// `http://<hostname>.local:3000/` resolves without knowing its IP.
#[embassy_executor::task]
pub async fn mdns_task(stack: Stack<'static>) {
    stack.wait_config_up().await;
    let Some(cfg) = stack.config_v4() else { return };
    let ip = cfg.address.address();
    let oct = ip.octets();

    let hostname = format!("{}-{:02x}{:02x}", slugify(DEVICE_NAME), oct[2], oct[3]);
    let instance = String::from(DEVICE_NAME);
    log::info!("debug-inspect: mdns hostname={}.local instance={}", hostname, instance);

    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1500];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_buf = [0u8; 1500];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx_buf, &mut tx_meta, &mut tx_buf);

    if let Err(e) = stack.join_multicast_group(MDNS_IP) {
        log::warn!("debug-inspect: mdns join_multicast_group failed: {:?}", e);
    }
    if socket.bind(5353).is_err() {
        log::warn!("debug-inspect: mdns failed to bind UDP 5353");
        return;
    }

    let mdns_ep = IpEndpoint::new(MDNS_IP, 5353);
    let announcement = build_mdns_announcement(&hostname, &instance, oct);
    let _ = socket.send_to(&announcement, mdns_ep).await;

    let mut next_announce = Instant::now() + Duration::from_secs(30);
    let mut recv_pkt = [0u8; 512];
    loop {
        let now = Instant::now();
        let remaining = if now >= next_announce {
            Duration::from_millis(1)
        } else {
            next_announce - now
        };
        let timed_out = match with_timeout(remaining, socket.recv_from(&mut recv_pkt)).await {
            Err(_) => true,
            Ok(Ok((n, _))) => {
                if let Some(resp) = handle_mdns_query(&recv_pkt[..n], &hostname, &instance, oct) {
                    let _ = socket.send_to(&resp, mdns_ep).await;
                }
                false
            }
            Ok(Err(_)) => break,
        };
        if timed_out || Instant::now() >= next_announce {
            let pkt = build_mdns_announcement(&hostname, &instance, oct);
            let _ = socket.send_to(&pkt, mdns_ep).await;
            next_announce = Instant::now() + Duration::from_secs(30);
        }
    }
}

// ── I/O helper ────────────────────────────────────────────────────────────

/// `embassy-net`'s `TcpSocket::write` may return fewer bytes than requested.
async fn write_all(sock: &mut TcpSocket<'_>, mut data: &[u8]) -> Result<(), ()> {
    while !data.is_empty() {
        match sock.write(data).await {
            Ok(0) => return Err(()),
            Ok(n) => data = &data[n..],
            Err(_) => return Err(()),
        }
    }
    Ok(())
}

// ── TCP accept loop ─────────────────────────────────────────────────────────

#[embassy_executor::task]
pub async fn debug_server(stack: Stack<'static>, state: &'static SharedInspectState) {
    let mut rx_storage = vec![0u8; 2048];
    let mut tx_storage = vec![0u8; 2048];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_storage, &mut tx_storage);
        socket.set_timeout(Some(Duration::from_secs(60)));

        log::info!("debug-inspect: waiting for a connection on port {}", PORT);
        if socket.accept(PORT).await.is_err() {
            Timer::after(Duration::from_millis(200)).await;
            continue;
        }

        log::info!("debug-inspect: client connected");
        handle_connection(&mut socket, state).await;

        socket.close();
        socket.flush().await.ok();
        socket.abort();
        log::info!("debug-inspect: client disconnected");
    }
}

// ── HTTP + WebSocket connection handler ─────────────────────────────────────

async fn handle_connection(socket: &mut TcpSocket<'_>, state: &'static SharedInspectState) {
    let mut http_buf = vec![0u8; 1536];
    let mut http_len = 0usize;

    // Read until end-of-headers marker.
    loop {
        match with_timeout(Duration::from_secs(10), socket.read(&mut http_buf[http_len..])).await
        {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => return,
            Ok(Ok(n)) => {
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
            write_all(socket, &resp_buf[..n]).await.ok();
            log::debug!("debug-inspect: WebSocket upgrade OK");
            run_ws_session(&mut ws, socket, state).await;
        }
        _ => {
            // Plain HTTP — serve the browser UI.
            let header = alloc::format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                INDEX_HTML.len()
            );
            write_all(socket, header.as_bytes()).await.ok();
            write_all(socket, INDEX_HTML).await.ok();
        }
    }
}

// ── WebSocket session ────────────────────────────────────────────────────────

/// Dispatch the message kinds that need no cross-task call and no metric/
/// command state owned by `run_ws_session` — `Hello`, `GetSchema`, `GetValue`,
/// `GetCommands`. `InvokeCommand`/`SetValue`/`SubscribeMetrics`/
/// `UnsubscribeMetrics` are matched directly in `run_ws_session` since they
/// need to `.await` a cross-task reply or mutate per-connection state;
/// `GetScreenshot` likewise (stage 5, still a placeholder below).
fn handle_msg(json: &str, state: &'static SharedInspectState, schema_json: &str) -> String {
    match parse_msg(json) {
        InMsg::Hello { request_id } => {
            log::info!("debug-inspect: Hello request_id={}", request_id);
            hello_ack(request_id, DEVICE_NAME, DEVICE_TYPE_STR, FIRMWARE_VERSION, "")
        }
        InMsg::GetSchema { request_id } => schema_resp(request_id, schema_json),
        InMsg::GetValue { request_id, path } => {
            let result = state.lock(|cell| {
                let s = cell.borrow();
                s.get_field_path(path).map(debug_value_to_json)
            });
            match result {
                Some(v) => value_resp(request_id, path, &v),
                None => error_resp(
                    request_id,
                    "UnknownPath",
                    &alloc::format!("no field at path: {}", path),
                ),
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

async fn run_ws_session(
    ws: &mut WebSocketServer,
    sock: &mut TcpSocket<'_>,
    state: &'static SharedInspectState,
) {
    // Schema is purely structural ('static field metadata) — build from a
    // default value so we never hold the critical section during recursive
    // allocation (isolation rule 1).
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

    // Metric subscription state — per-connection, owned by this WS session.
    let mut subscribed_metric_paths: Vec<String> = Vec::new();
    let mut metric_interval_ms: u32 = 0;
    let mut last_metric: Option<Instant> = None;

    'outer: loop {
        // Push MetricBatch when subscriptions are active and the interval elapsed.
        if !subscribed_metric_paths.is_empty() && metric_interval_ms > 0 {
            let should_sample = match last_metric {
                None => true,
                Some(t) => Instant::now() - t >= Duration::from_millis(metric_interval_ms as u64),
            };
            if should_sample {
                let ts = Instant::now().as_millis();
                let snap: InspectState = state.lock(|cell| cell.borrow().clone());
                let samples: Vec<(String, f64)> = subscribed_metric_paths
                    .iter()
                    .filter_map(|path| {
                        snap.get_field_path(path)
                            .and_then(debug_value_to_f64)
                            .map(|v| (path.clone(), v))
                    })
                    .collect();
                if !samples.is_empty() {
                    let json = metric_batch_json(ts, metric_interval_ms, &samples);
                    let n = ws
                        .write(WebSocketSendMessageType::Text, true, json.as_bytes(), &mut tx_buf)
                        .unwrap_or(0);
                    if n > 0 && write_all(sock, &tx_buf[..n]).await.is_err() {
                        break 'outer;
                    }
                }
                last_metric = Some(Instant::now());
            }
        }

        // Push ValueChanged events every 2 s. Take one snapshot under a single
        // brief critical section (isolation rule 2), then do all comparison
        // and I/O outside the lock so interrupts stay enabled.
        if Instant::now() - last_event >= Duration::from_secs(2) {
            let snap: InspectState = state.lock(|cell| cell.borrow().clone());
            for (i, path) in leaf_paths.iter().enumerate() {
                if let Some(val) = snap.get_field_path(path).map(debug_value_to_json) {
                    if snapshot[i] != val {
                        snapshot[i] = val.clone();
                        let msg = changed_resp(path, &val, seq);
                        seq += 1;
                        let n = ws
                            .write(WebSocketSendMessageType::Text, true, msg.as_bytes(), &mut tx_buf)
                            .unwrap_or(0);
                        if n > 0 && write_all(sock, &tx_buf[..n]).await.is_err() {
                            break 'outer;
                        }
                    }
                }
            }
            last_event = Instant::now();
        }

        let elapsed = Instant::now() - last_event;
        let remaining = if elapsed < Duration::from_secs(2) {
            Duration::from_secs(2) - elapsed
        } else {
            Duration::from_millis(20)
        };
        let remaining_metric = if !subscribed_metric_paths.is_empty() && metric_interval_ms > 0 {
            match last_metric {
                None => Duration::from_millis(1),
                Some(t) => {
                    let e = Instant::now() - t;
                    let interval = Duration::from_millis(metric_interval_ms as u64);
                    if e >= interval {
                        Duration::from_millis(10)
                    } else {
                        interval - e
                    }
                }
            }
        } else {
            Duration::from_secs(3600)
        };
        let remaining = remaining.min(remaining_metric);

        match with_timeout(remaining, sock.read(&mut rx_buf[buf_used..])).await {
            Ok(Ok(0)) | Ok(Err(_)) => break,
            Err(_timeout) => continue,
            Ok(Ok(n)) => buf_used += n,
        }

        'inner: loop {
            if buf_used == 0 {
                break 'inner;
            }
            match ws.read(&rx_buf[..buf_used], &mut pl_buf) {
                Err(_) => break 'outer,
                Ok(r) if r.len_from == 0 => break 'inner,
                Ok(r) => {
                    // Shift buffer before any await so we don't borrow rx_buf across awaits.
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
                                    let n = ws
                                        .write(WebSocketSendMessageType::Text, true, msg.as_bytes(), &mut tx_buf)
                                        .unwrap_or(0);
                                    if write_all(sock, &tx_buf[..n]).await.is_err() {
                                        break 'outer;
                                    }
                                    break 'inner;
                                }
                            };
                            match parse_msg(json) {
                                InMsg::GetScreenshot { request_id } => {
                                    if !send_screenshot(ws, sock, request_id, &mut tx_buf).await {
                                        break 'outer;
                                    }
                                    // After streaming (which awaits repeatedly), re-poll
                                    // the socket from the outer loop rather than trying
                                    // to keep parsing whatever's left in rx_buf.
                                    break 'inner;
                                }
                                InMsg::InvokeCommand { request_id, name, args_json } => {
                                    let args = parse_command_args(args_json);
                                    CMD_CHANNEL
                                        .send(CommandRequest { request_id, name: String::from(name), args })
                                        .await;
                                    let reply = match with_timeout(Duration::from_secs(5), CMD_RESP.wait()).await {
                                        Ok(resp) => command_result_json(resp.request_id, &resp.output, resp.duration_ms),
                                        Err(_) => command_result_json(
                                            request_id,
                                            &CommandOutput::Error(String::from("timeout: device busy")),
                                            0,
                                        ),
                                    };
                                    let n = ws
                                        .write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf)
                                        .unwrap_or(0);
                                    if write_all(sock, &tx_buf[..n]).await.is_err() {
                                        break 'outer;
                                    }
                                }
                                InMsg::SetValue { request_id, path, value_json } => {
                                    let reply = match parse_set_value(value_json) {
                                        None => set_value_error(request_id, "MalformedRequest", path),
                                        Some(value) => {
                                            SET_CHANNEL
                                                .send(SetValueRequest { request_id, path: String::from(path), value })
                                                .await;
                                            match with_timeout(Duration::from_secs(5), SET_RESP.wait()).await {
                                                Ok((_, SetValueResult::Ok)) => set_value_ack(request_id, path),
                                                Ok((_, SetValueResult::ReadOnly)) => {
                                                    set_value_error(request_id, "ReadOnly", path)
                                                }
                                                Ok((_, SetValueResult::TypeMismatch)) => {
                                                    set_value_error(request_id, "TypeMismatch", path)
                                                }
                                                Ok((_, SetValueResult::OutOfBounds)) => {
                                                    set_value_error(request_id, "OutOfBounds", path)
                                                }
                                                Ok((_, SetValueResult::UnknownField)) => {
                                                    set_value_error(request_id, "UnknownPath", path)
                                                }
                                                Ok((_, SetValueResult::UnknownVariant)) => {
                                                    set_value_error(request_id, "UnknownVariant", path)
                                                }
                                                Err(_) => set_value_error(request_id, "Timeout", path),
                                            }
                                        }
                                    };
                                    let n = ws
                                        .write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf)
                                        .unwrap_or(0);
                                    if write_all(sock, &tx_buf[..n]).await.is_err() {
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
                                    last_metric = None; // trigger an immediate first batch
                                    let reply = subscribe_metrics_ack_json(
                                        request_id,
                                        effective_interval,
                                        &subscribed_metric_paths,
                                    );
                                    let n = ws
                                        .write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf)
                                        .unwrap_or(0);
                                    if write_all(sock, &tx_buf[..n]).await.is_err() {
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
                                    let reply =
                                        unsubscribe_metrics_ack_json(request_id, &subscribed_metric_paths);
                                    let n = ws
                                        .write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf)
                                        .unwrap_or(0);
                                    if write_all(sock, &tx_buf[..n]).await.is_err() {
                                        break 'outer;
                                    }
                                }
                                _ => {
                                    let reply = handle_msg(json, state, &schema_json);
                                    let n = ws
                                        .write(WebSocketSendMessageType::Text, true, reply.as_bytes(), &mut tx_buf)
                                        .unwrap_or(0);
                                    if write_all(sock, &tx_buf[..n]).await.is_err() {
                                        break 'outer;
                                    }
                                }
                            }
                        }
                        WebSocketReceiveMessageType::CloseMustReply => {
                            let n = ws
                                .close(WebSocketCloseStatusCode::NormalClosure, None, &mut tx_buf)
                                .unwrap_or(0);
                            write_all(sock, &tx_buf[..n]).await.ok();
                            break 'outer;
                        }
                        WebSocketReceiveMessageType::Ping => {
                            let n = ws
                                .write(WebSocketSendMessageType::Pong, true, payload, &mut tx_buf)
                                .unwrap_or(0);
                            if write_all(sock, &tx_buf[..n]).await.is_err() {
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
