//! Transport-agnostic core of the `debug-inspect` WebSocket debug server:
//! the reflectable [`InspectState`] tree, its remote commands, and all the
//! hand-rolled JSON schema/message encode-decode logic. Nothing in this file
//! references a socket, an async runtime, or a lock type — `inspect_esp.rs`
//! and `inspect_sim.rs` each provide their own transport around it.
//!
//! Ported from the proven reference implementation at
//! `../epaper-examples/examples/inspect_demo.rs`.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use embedded_inspect::{
    debug_commands, CommandArg, CommandDef, CommandOutput, CommandParamKind, CommandReturnKind,
    DebugInspect, DebugValue, Inspect, TypeSchema, ValueKind,
};

// ── InspectState ────────────────────────────────────────────────────────────
//
// A separate, small, `Clone`-able reflectable struct — never the real
// `AppState`/`HardwareAccess`. It is populated by `sync_from_real_state`
// (see `inspect_esp.rs`/`inspect_sim.rs`) each tick, and read back by the
// owning task/loop to apply remote writes/commands via the plain "mailbox"
// fields below (no `#[inspect]` attribute -> invisible to the schema, exactly
// like the derive's own `_secret`-style convention).

#[derive(Debug, Clone, Default, DebugInspect)]
pub struct NetworkState {
    #[inspect(read_only)]
    pub connected: bool,
    #[inspect(read_only)]
    pub ip_a: u8,
    #[inspect(read_only)]
    pub ip_b: u8,
    #[inspect(read_only)]
    pub ip_c: u8,
    #[inspect(read_only)]
    pub ip_d: u8,
}

#[derive(Debug, Clone, Default, DebugInspect)]
pub struct SystemState {
    #[inspect(read_only, metric, unit = "s")]
    pub uptime_secs: u32,
    #[inspect(read_only, metric, unit = "B")]
    pub free_heap_bytes: u32,
    #[inspect(read_only, metric)]
    pub battery_percent: u8,
    #[inspect(read_only, metric, unit = "mV")]
    pub battery_voltage_mv: u32,
    #[inspect(read_only)]
    pub battery_charging: bool,
}

#[derive(Debug, Clone, Default, DebugInspect)]
pub struct BookState {
    #[inspect(read_only)]
    pub current_filename: String,
    #[inspect(read_only)]
    pub chapter_idx: u32,
    #[inspect(read_only)]
    pub anchor_byte: u32,
    #[inspect(read_only, metric)]
    pub partial_refresh_count: u32,
    #[inspect(read_only, metric)]
    pub full_quality_count: u32,
}

#[derive(Debug, Clone, Default, DebugInspect)]
pub struct SettingsState {
    #[inspect(read_only)]
    pub orientation: String,
    #[inspect(read_only)]
    pub font_size: String,
    #[inspect(read_only)]
    pub backlight_level: String,
    #[inspect(read_only)]
    pub utc_offset_minutes: i32,
}

#[derive(Debug, Clone, Default, DebugInspect)]
pub struct InspectState {
    #[inspect(read_only)]
    pub network: NetworkState,
    #[inspect(read_only)]
    pub system: SystemState,
    #[inspect(read_only)]
    pub book: BookState,
    #[inspect(read_only)]
    pub settings: SettingsState,

    // Mailbox fields — plain (no `#[inspect]` attribute -> invisible to the
    // schema), used to carry remote-command intent back to the task/loop that
    // owns real `AppState`/`HardwareAccess`, without ever handing the debug
    // server a `&mut` into real state directly. Drained and cleared once
    // applied — see `apply_pending_commands` in `inspect_esp.rs`/`inspect_sim.rs`.
    pub pending_ntp_sync: bool,
    pub pending_full_refresh: bool,
    pub pending_reset_counters: bool,
    pub pending_font_size: Option<String>,
    pub pending_backlight: Option<String>,
    pub pending_orientation: Option<String>,
}

#[debug_commands]
impl InspectState {
    // Note: this only sets a mailbox flag, not `self.book.*` directly —
    // those fields get overwritten from real state every tick by
    // `sync_from_real_state`, so mutating them here would just be
    // immediately clobbered. The owning task/loop clears the real counters
    // and this flag together when it drains the mailbox.
    #[debug_command]
    fn reset_counters(&mut self) {
        self.pending_reset_counters = true;
    }

    #[debug_command]
    fn request_ntp_sync(&mut self) {
        self.pending_ntp_sync = true;
    }

    #[debug_command]
    fn force_full_refresh(&mut self) {
        self.pending_full_refresh = true;
    }

    // Note: these take `&str`, not `String` — `embedded-inspect-derive`'s
    // `#[debug_command]` parameter codegen has an ordering bug where a bare
    // `String` parameter is misclassified as an inspectable enum type before
    // its dedicated `String` handling ever runs (field-level `String` support,
    // used above, is unaffected — only command *parameters* hit this). `&str`
    // takes a different, correctly-ordered code path in the same macro.
    #[debug_command]
    fn set_font_size(&mut self, size: &str) {
        self.pending_font_size = Some(String::from(size));
    }

    #[debug_command]
    fn set_backlight(&mut self, level: &str) {
        self.pending_backlight = Some(String::from(level));
    }

    #[debug_command]
    fn set_orientation(&mut self, orientation: &str) {
        self.pending_orientation = Some(String::from(orientation));
    }
}

// ── Real-state sync / mailbox application (shared by both targets) ─────────
//
// Neither of these touches a socket, a lock type, or an embassy type — they
// operate purely on `InspectState` + the app's real `AppState`/
// `HardwareAccess`, which both `esp` and `simulator` builds already have.
// Each transport (`inspect_esp.rs`/`inspect_sim.rs`) is responsible for its
// own locking around these calls and for draining its own command/set-value
// channel before calling `apply_mailbox`.

/// Copies real device state into `dst`. Called once per tick by the task/loop
/// that owns `app`/`hw` — never call this while already holding `dst`'s lock
/// on the caller's side twice (one lock-and-write per tick, isolation rule 2).
pub fn sync_from_real_state(
    dst: &mut InspectState,
    app: &crate::appstate::AppState,
    hw: &dyn crate::hardware::HardwareAccess,
    uptime_secs: u32,
) {
    let bat = hw.battery_info();
    dst.system.uptime_secs = uptime_secs;
    dst.system.battery_percent = bat.percent;
    dst.system.battery_voltage_mv = bat.voltage_mv;
    dst.system.battery_charging = bat.is_charging;
    dst.system.free_heap_bytes = hw.memory_info().sram_free_bytes as u32;

    dst.book.current_filename = String::from(app.current_filename.as_str());
    dst.book.chapter_idx = app.session.chapter_idx as u32;
    dst.book.anchor_byte = app.session.reader.anchor_byte as u32;
    dst.book.partial_refresh_count = app.partial_refresh_count;
    dst.book.full_quality_count = app.full_quality_count;

    dst.settings.orientation = format!("{:?}", hw.orientation());
    dst.settings.font_size = format!("{:?}", hw.font_size());
    dst.settings.backlight_level = format!("{:?}", hw.backlight_level());
    dst.settings.utc_offset_minutes = hw.utc_offset_minutes();
}

fn parse_font_size(s: &str) -> Option<crate::hardware::FontSize> {
    use crate::hardware::FontSize::*;
    match s {
        "Small" => Some(Small),
        "Medium" => Some(Medium),
        "Large" => Some(Large),
        _ => None,
    }
}
fn parse_backlight(s: &str) -> Option<crate::hardware::BacklightLevel> {
    use crate::hardware::BacklightLevel::*;
    match s {
        "Off" => Some(Off),
        "Low" => Some(Low),
        "High" => Some(High),
        _ => None,
    }
}
fn parse_orientation(s: &str) -> Option<crate::hardware::Orientation> {
    use crate::hardware::Orientation::*;
    match s {
        "Portrait" => Some(Portrait),
        "Landscape" => Some(Landscape),
        "ReversePortrait" => Some(ReversePortrait),
        "ReverseLandscape" => Some(ReverseLandscape),
        _ => None,
    }
}

/// Effects `apply_mailbox` couldn't perform itself because they need
/// something the caller has and this function doesn't:
/// - `ntp_sync_requested`: the ESP transport's `WIFI_SYNC_REQUEST` signal
///   lives in the `examples/` binary, not this library.
/// - `orientation_changed`: changing orientation also means updating the
///   display bridge's rotation and resizing the scene — both live in
///   `examples/ereader_ui.rs`, not in `AppState`/`HardwareAccess`. The
///   caller must apply it the same way the existing settings-dialog
///   orientation click handler does (see `ORIENTATION_ID` handling in
///   `examples/ereader_ui.rs`).
#[derive(Default)]
pub struct MailboxEffects {
    pub ntp_sync_requested: bool,
    pub orientation_changed: Option<crate::hardware::Orientation>,
}

/// Drains the mailbox fields a dispatched command/set-value left on
/// `state` (see `InspectState`'s `#[debug_commands]` impl above) and applies
/// them to the real `AppState`/`HardwareAccess`, clearing each flag as it's
/// consumed. Reuses the same `FontSize`/`BacklightLevel`/`Orientation`
/// parsing round-trip that `sync_from_real_state` writes (`{:?}`-formatted
/// full variant names), not the settings-dialog's `from_cmd` abbreviations
/// (`"Land"`, `"R.Port"`, ...) which are a UI-button-label concern, not a
/// wire-protocol one.
pub fn apply_mailbox(
    state: &mut InspectState,
    app: &mut crate::appstate::AppState,
    hw: &mut dyn crate::hardware::HardwareAccess,
) -> MailboxEffects {
    let ntp_sync = core::mem::take(&mut state.pending_ntp_sync);
    let full_refresh = core::mem::take(&mut state.pending_full_refresh);
    let reset_counters = core::mem::take(&mut state.pending_reset_counters);
    let font_size = state.pending_font_size.take();
    let backlight = state.pending_backlight.take();
    let orientation = state.pending_orientation.take();

    if full_refresh {
        // Forces the next page-turn refresh to use the full 15-frame quality
        // waveform instead of the fast 4-frame one.
        app.full_quality_count = u32::MAX;
    }
    if reset_counters {
        app.partial_refresh_count = 0;
        app.full_quality_count = 0;
    }
    if let Some(size) = font_size.as_deref().and_then(parse_font_size) {
        hw.set_font_size(size);
        app.cfg = crate::appstate::cfg_from_scene(&mut app.scene, &app.theme, &app.fonts, hw.font_size());
        app.session.reader.relayout(&app.cfg);
        app.update_content(hw);
        hw.save_settings();
    }
    if let Some(level) = backlight.as_deref().and_then(parse_backlight) {
        hw.set_backlight_level(level);
        hw.save_settings();
    }

    MailboxEffects {
        ntp_sync_requested: ntp_sync,
        orientation_changed: orientation.as_deref().and_then(parse_orientation),
    }
}

// ── Structured logging (plain data — enqueue mechanism is per-target) ──────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub struct LogEntry {
    pub timestamp_ms: u64,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

pub fn log_level_str(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Trace => "trace",
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

// ── Schema building ─────────────────────────────────────────────────────────

enum SchemaNode {
    Struct {
        type_name: String,
        fields: Vec<FieldNode>,
    },
    Enum {
        type_name: String,
        variants: Vec<String>,
    },
    Primitive {
        primitive: String,
    },
}

struct FieldNode {
    name: String,
    read_only: bool,
    min_val: Option<f64>,
    max_val: Option<f64>,
    metric: bool,
    unit: Option<&'static str>,
    schema: SchemaNode,
}

fn build_schema(inspect: &dyn Inspect) -> SchemaNode {
    match inspect.type_schema() {
        TypeSchema::Struct(s) => {
            let fields = s
                .fields
                .iter()
                .map(|f| {
                    let schema = if f.kind == ValueKind::Object {
                        inspect
                            .get_field(f.name)
                            .and_then(|v| {
                                if let DebugValue::Object(sub) = v {
                                    Some(build_schema(sub))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(SchemaNode::Primitive {
                                primitive: "object".into(),
                            })
                    } else {
                        SchemaNode::Primitive {
                            primitive: format!("{:?}", f.kind).to_lowercase(),
                        }
                    };
                    FieldNode {
                        name: f.name.into(),
                        read_only: f.read_only,
                        min_val: f.min_val,
                        max_val: f.max_val,
                        metric: f.metric,
                        unit: f.unit,
                        schema,
                    }
                })
                .collect();
            SchemaNode::Struct {
                type_name: s.type_name.into(),
                fields,
            }
        }
        TypeSchema::Enum(e) => SchemaNode::Enum {
            type_name: e.type_name.into(),
            variants: e.variants.iter().map(|v| String::from(*v)).collect(),
        },
    }
}

fn schema_to_json(node: &SchemaNode) -> String {
    match node {
        SchemaNode::Struct { type_name, fields } => {
            let field_jsons: Vec<String> = fields
                .iter()
                .map(|f| {
                    let min_json = match f.min_val {
                        Some(v) => format!(",\"min_val\":{}", v),
                        None => String::new(),
                    };
                    let max_json = match f.max_val {
                        Some(v) => format!(",\"max_val\":{}", v),
                        None => String::new(),
                    };
                    let metric_json = if f.metric { ",\"metric\":true" } else { "" };
                    let unit_json = match f.unit {
                        Some(u) => format!(",\"unit\":\"{}\"", u),
                        None => String::new(),
                    };
                    format!(
                        r#"{{"name":"{}","read_only":{}{}{}{}{},"schema":{}}}"#,
                        f.name,
                        f.read_only,
                        min_json,
                        max_json,
                        metric_json,
                        unit_json,
                        schema_to_json(&f.schema)
                    )
                })
                .collect();
            format!(
                r#"{{"kind":"Struct","type_name":"{}","fields":[{}]}}"#,
                type_name,
                field_jsons.join(",")
            )
        }
        SchemaNode::Enum { type_name, variants } => {
            let var_jsons: Vec<String> = variants.iter().map(|v| format!("\"{}\"", v)).collect();
            format!(
                r#"{{"kind":"Enum","type_name":"{}","variants":[{}]}}"#,
                type_name,
                var_jsons.join(",")
            )
        }
        SchemaNode::Primitive { primitive } => {
            format!(r#"{{"kind":"Primitive","primitive":"{}"}}"#, primitive)
        }
    }
}

/// Build and JSON-encode the schema for `InspectState` in one call — always
/// built from `InspectState::default()` by the caller (never from a locked
/// live instance), per isolation rule 1.
pub fn inspect_state_schema_json() -> String {
    schema_to_json(&build_schema(&InspectState::default()))
}

pub fn debug_value_to_json(v: DebugValue<'_>) -> String {
    match v {
        DebugValue::Bool(b) => format!("{}", b),
        DebugValue::U8(n) => format!("{}", n),
        DebugValue::U16(n) => format!("{}", n),
        DebugValue::U32(n) => format!("{}", n),
        DebugValue::U64(n) => format!("{}", n),
        DebugValue::U128(n) => format!("\"{}\"", n),
        DebugValue::I8(n) => format!("{}", n),
        DebugValue::I16(n) => format!("{}", n),
        DebugValue::I32(n) => format!("{}", n),
        DebugValue::I64(n) => format!("{}", n),
        DebugValue::I128(n) => format!("\"{}\"", n),
        DebugValue::F32(n) => format!("{}", n),
        DebugValue::F64(n) => format!("{}", n),
        DebugValue::Str(s) => format!("\"{}\"", json_escape(s)),
        DebugValue::Object(o) => match o.type_schema() {
            TypeSchema::Enum(_) => format!(
                "{{\"variant\":\"{}\"}}",
                o.active_variant().unwrap_or("unknown")
            ),
            TypeSchema::Struct(s) => format!("{{\"object\":\"{}\"}}", s.type_name),
        },
    }
}

pub fn debug_value_to_f64(v: DebugValue<'_>) -> Option<f64> {
    match v {
        DebugValue::U8(n) => Some(n as f64),
        DebugValue::U16(n) => Some(n as f64),
        DebugValue::U32(n) => Some(n as f64),
        DebugValue::U64(n) => Some(n as f64),
        DebugValue::U128(n) => Some(n as f64),
        DebugValue::I8(n) => Some(n as f64),
        DebugValue::I16(n) => Some(n as f64),
        DebugValue::I32(n) => Some(n as f64),
        DebugValue::I64(n) => Some(n as f64),
        DebugValue::I128(n) => Some(n as f64),
        DebugValue::F32(n) => Some(n as f64),
        DebugValue::F64(n) => Some(n),
        _ => None,
    }
}

pub fn collect_leaf_paths(inspect: &dyn Inspect, prefix: &str, paths: &mut Vec<String>) {
    collect_leaf_paths_node(&build_schema(inspect), prefix, paths);
}

fn collect_leaf_paths_node(node: &SchemaNode, prefix: &str, paths: &mut Vec<String>) {
    match node {
        SchemaNode::Struct { fields, .. } => {
            for f in fields {
                let path = if prefix.is_empty() {
                    f.name.clone()
                } else {
                    format!("{}.{}", prefix, f.name)
                };
                collect_leaf_paths_node(&f.schema, &path, paths);
            }
        }
        _ => {
            if !prefix.is_empty() {
                paths.push(String::from(prefix));
            }
        }
    }
}

// ── Minimal JSON field extraction ───────────────────────────────────────────

fn json_str_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\":\"", key);
    let start = json.find(needle.as_str())? + needle.len();
    let len = json[start..].find('"')?;
    Some(&json[start..start + len])
}

fn json_bool_field(json: &str, key: &str) -> bool {
    let needle = format!("\"{}\":", key);
    let rest = match json.find(needle.as_str()) {
        Some(p) => &json[p + needle.len()..],
        None => return false,
    };
    let rest = rest.trim_start();
    rest.starts_with("true")
}

fn json_raw_array_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\":[", key);
    let start = json.find(needle.as_str())? + needle.len() - 1;
    let rest = &json[start..];
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in rest.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && in_str {
            escape = true;
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if c == '[' {
            depth += 1;
        } else if c == ']' {
            depth -= 1;
            if depth == 0 {
                return Some(&rest[..=i]);
            }
        }
    }
    None
}

fn json_raw_object_field<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{}\":{{", key);
    let start = json.find(needle.as_str())? + needle.len() - 1;
    let rest = &json[start..];
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in rest.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && in_str {
            escape = true;
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(&rest[..=i]);
            }
        }
    }
    None
}

fn json_u32_field(json: &str, key: &str) -> Option<u32> {
    let needle = format!("\"{}\":", key);
    let rest = &json[json.find(needle.as_str())? + needle.len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn find_matching_brace(s: &str) -> usize {
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    for (i, c) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        if c == '\\' && in_str {
            escape = true;
            continue;
        }
        if c == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if c == '{' {
            depth += 1;
        } else if c == '}' {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    s.len().saturating_sub(1)
}

/// Escapes a string for embedding inside a JSON `"…"` value.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use core::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = String::from(out.trim_end_matches('-'));
    if trimmed.is_empty() {
        String::from("device")
    } else {
        trimmed
    }
}

pub fn wrapping_checksum(data: &[u8]) -> u32 {
    data.iter().fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
}

// ── Protocol messages ────────────────────────────────────────────────────────

pub enum InMsg<'a> {
    Hello {
        request_id: u32,
    },
    GetSchema {
        request_id: u32,
    },
    GetValue {
        request_id: u32,
        path: &'a str,
    },
    GetScreenshot {
        request_id: u32,
    },
    GetCommands {
        request_id: u32,
    },
    InvokeCommand {
        request_id: u32,
        name: &'a str,
        args_json: &'a str,
    },
    SetValue {
        request_id: u32,
        path: &'a str,
        value_json: &'a str,
    },
    SubscribeMetrics {
        request_id: u32,
        paths_json: &'a str,
        interval_ms: u32,
    },
    UnsubscribeMetrics {
        request_id: u32,
        paths_json: &'a str,
    },
    Unknown,
}

pub fn parse_msg(json: &str) -> InMsg<'_> {
    let request_id = json_u32_field(json, "request_id").unwrap_or(0);
    match json_str_field(json, "type") {
        Some("Hello") => InMsg::Hello { request_id },
        Some("GetSchema") => InMsg::GetSchema { request_id },
        Some("GetValue") => InMsg::GetValue {
            request_id,
            path: json_str_field(json, "path").unwrap_or(""),
        },
        Some("GetScreenshot") => InMsg::GetScreenshot { request_id },
        Some("GetCommands") => InMsg::GetCommands { request_id },
        Some("InvokeCommand") => InMsg::InvokeCommand {
            request_id,
            name: json_str_field(json, "name").unwrap_or(""),
            args_json: json_raw_array_field(json, "args").unwrap_or("[]"),
        },
        Some("SetValue") => InMsg::SetValue {
            request_id,
            path: json_str_field(json, "path").unwrap_or(""),
            value_json: json_raw_object_field(json, "value").unwrap_or("{}"),
        },
        Some("SubscribeMetrics") => InMsg::SubscribeMetrics {
            request_id,
            paths_json: json_raw_array_field(json, "paths").unwrap_or("[]"),
            interval_ms: json_u32_field(json, "interval_ms").unwrap_or(1000),
        },
        Some("UnsubscribeMetrics") => InMsg::UnsubscribeMetrics {
            request_id,
            paths_json: json_raw_array_field(json, "paths").unwrap_or("[]"),
        },
        _ => InMsg::Unknown,
    }
}

pub fn parse_command_args(args_json: &str) -> Vec<CommandArg> {
    // args_json: [{"kind":"u32","value":42},{"kind":"bool","value":true},...]
    let mut result = Vec::new();
    let mut rest = args_json.trim();
    if !rest.starts_with('[') {
        return result;
    }
    rest = &rest[1..];
    loop {
        rest = rest.trim_start_matches(|c: char| c == ',' || c.is_ascii_whitespace());
        if rest.is_empty() || rest.starts_with(']') {
            break;
        }
        if !rest.starts_with('{') {
            break;
        }
        let end = find_matching_brace(rest);
        let obj = &rest[..=end];
        rest = &rest[end + 1..];
        if let Some(arg) = parse_command_arg_obj(obj) {
            result.push(arg);
        }
    }
    result
}

fn parse_command_arg_obj(obj: &str) -> Option<CommandArg> {
    let kind = json_str_field(obj, "kind").unwrap_or("");
    match kind {
        "bool" => Some(CommandArg::Bool(json_bool_field(obj, "value"))),
        "u8" => json_u32_field(obj, "value").map(|v| CommandArg::U8(v as u8)),
        "u16" => json_u32_field(obj, "value").map(|v| CommandArg::U16(v as u16)),
        "u32" => json_u32_field(obj, "value").map(CommandArg::U32),
        "u64" => json_u32_field(obj, "value").map(|v| CommandArg::U64(v as u64)),
        "i8" => json_u32_field(obj, "value").map(|v| CommandArg::I8(v as i8)),
        "i16" => json_u32_field(obj, "value").map(|v| CommandArg::I16(v as i16)),
        "i32" => json_u32_field(obj, "value").map(|v| CommandArg::I32(v as i32)),
        "i64" => json_u32_field(obj, "value").map(|v| CommandArg::I64(v as i64)),
        "f32" => json_u32_field(obj, "value").map(|v| CommandArg::F32(v as f32)),
        "f64" => json_u32_field(obj, "value").map(|v| CommandArg::F64(v as f64)),
        "str" | "string" => json_str_field(obj, "value").map(|s| CommandArg::Str(s.into())),
        "enum" => json_str_field(obj, "value").map(|s| CommandArg::Enum(s.into())),
        _ => None,
    }
}

pub fn parse_set_value(value_json: &str) -> Option<CommandArg> {
    parse_command_arg_obj(value_json)
}

pub fn parse_string_array(json: &str) -> Vec<String> {
    let mut result = Vec::new();
    let json = json.trim();
    if !json.starts_with('[') {
        return result;
    }
    let mut rest = &json[1..];
    loop {
        rest = rest.trim_start_matches(|c: char| c == ',' || c.is_ascii_whitespace());
        if rest.is_empty() || rest.starts_with(']') {
            break;
        }
        if !rest.starts_with('"') {
            break;
        }
        rest = &rest[1..];
        if let Some(end) = rest.find('"') {
            result.push(rest[..end].into());
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
    result
}

// ── Response builders ────────────────────────────────────────────────────────

pub fn hello_ack(
    rid: u32,
    device_name: &str,
    device_type: &str,
    firmware_version: &str,
    mdns_hostname: &str,
) -> String {
    format!(
        r#"{{"type":"HelloAck","request_id":{},"version":1,"server_name":"ereader-inspect","device_name":"{}","device_type":"{}","firmware_version":"{}","mdns_hostname":"{}"}}"#,
        rid,
        json_escape(device_name),
        json_escape(device_type),
        json_escape(firmware_version),
        json_escape(mdns_hostname),
    )
}

pub fn schema_resp(rid: u32, schema_json: &str) -> String {
    format!(
        r#"{{"type":"SchemaResponse","request_id":{},"schema":{}}}"#,
        rid, schema_json
    )
}

pub fn value_resp(rid: u32, path: &str, value_json: &str) -> String {
    format!(
        r#"{{"type":"ResponseValue","request_id":{},"path":"{}","value":{}}}"#,
        rid,
        json_escape(path),
        value_json
    )
}

pub fn error_resp(rid: u32, code: &str, msg: &str) -> String {
    format!(
        r#"{{"type":"Error","request_id":{},"code":"{}","message":"{}"}}"#,
        rid,
        json_escape(code),
        json_escape(msg)
    )
}

pub fn changed_resp(path: &str, value_json: &str, seq: u32) -> String {
    format!(
        r#"{{"type":"ValueChanged","path":"{}","value":{},"sequence":{}}}"#,
        json_escape(path),
        value_json,
        seq
    )
}

/// `format` is a caller-supplied wire-format tag (e.g. `"Gray4"` on ESP,
/// `"RGB565LE"` on the simulator) since the two targets' framebuffers are
/// genuinely different pixel formats — see `inspect_esp.rs`/`inspect_sim.rs`.
pub fn screenshot_begin_resp(
    capture_id: u32,
    width: u16,
    height: u16,
    format_tag: &str,
    total_bytes: u32,
    chunk_size: u16,
    total_chunks: u32,
) -> String {
    format!(
        r#"{{"type":"ScreenshotBegin","capture_id":{},"width":{},"height":{},"format":"{}","total_bytes":{},"chunk_size":{},"total_chunks":{}}}"#,
        capture_id, width, height, format_tag, total_bytes, chunk_size, total_chunks,
    )
}

pub fn screenshot_end_resp(capture_id: u32, total_chunks: u32, total_checksum: u32) -> String {
    format!(
        r#"{{"type":"ScreenshotEnd","capture_id":{},"total_chunks":{},"total_checksum":{}}}"#,
        capture_id, total_chunks, total_checksum,
    )
}

pub fn screenshot_unavailable_resp(rid: u32) -> String {
    error_resp(
        rid,
        "NoSnapshot",
        "no framebuffer snapshot available yet; wait for the next render cycle",
    )
}

pub fn log_entry_json(entry: &LogEntry, dropped_before: u32) -> String {
    format!(
        r#"{{"type":"LogRecord","timestamp_ms":{},"level":"{}","target":"{}","message":"{}","dropped_before":{}}}"#,
        entry.timestamp_ms,
        log_level_str(entry.level),
        json_escape(&entry.target),
        json_escape(&entry.message),
        dropped_before,
    )
}

fn command_param_kind_json(kind: &CommandParamKind) -> String {
    match kind {
        CommandParamKind::Bool => r#"{"type":"bool"}"#.into(),
        CommandParamKind::U8 => r#"{"type":"u8"}"#.into(),
        CommandParamKind::U16 => r#"{"type":"u16"}"#.into(),
        CommandParamKind::U32 => r#"{"type":"u32"}"#.into(),
        CommandParamKind::U64 => r#"{"type":"u64"}"#.into(),
        CommandParamKind::U128 => r#"{"type":"u128"}"#.into(),
        CommandParamKind::I8 => r#"{"type":"i8"}"#.into(),
        CommandParamKind::I16 => r#"{"type":"i16"}"#.into(),
        CommandParamKind::I32 => r#"{"type":"i32"}"#.into(),
        CommandParamKind::I64 => r#"{"type":"i64"}"#.into(),
        CommandParamKind::I128 => r#"{"type":"i128"}"#.into(),
        CommandParamKind::F32 => r#"{"type":"f32"}"#.into(),
        CommandParamKind::F64 => r#"{"type":"f64"}"#.into(),
        CommandParamKind::Str => r#"{"type":"str"}"#.into(),
        CommandParamKind::Enum { type_name, variants } => {
            let vs: Vec<String> = variants.iter().map(|v| format!("\"{}\"", v)).collect();
            format!(
                r#"{{"type":"enum","type_name":"{}","variants":[{}]}}"#,
                type_name,
                vs.join(",")
            )
        }
    }
}

pub fn commands_response_json(rid: u32, defs: &[CommandDef]) -> String {
    let cmds: Vec<String> = defs
        .iter()
        .map(|d| {
            let params: Vec<String> = d
                .params
                .iter()
                .map(|p| {
                    format!(
                        r#"{{"name":"{}","kind":{}}}"#,
                        p.name,
                        command_param_kind_json(&p.kind)
                    )
                })
                .collect();
            let rk = match d.return_kind {
                CommandReturnKind::Unit => "unit",
                CommandReturnKind::Result => "result",
            };
            let desc = match d.description {
                Some(s) => format!("\"{}\"", json_escape(s)),
                None => "null".into(),
            };
            format!(
                r#"{{"name":"{}","description":{},"params":[{}],"return_kind":"{}"}}"#,
                d.name,
                desc,
                params.join(","),
                rk
            )
        })
        .collect();
    format!(
        r#"{{"type":"CommandsResponse","request_id":{},"commands":[{}]}}"#,
        rid,
        cmds.join(",")
    )
}

pub fn command_result_json(rid: u32, output: &CommandOutput, duration_ms: u32) -> String {
    let output_json = match output {
        CommandOutput::Unit => r#"{"ok":true}"#.into(),
        CommandOutput::Error(msg) => format!(r#"{{"ok":false,"error":"{}"}}"#, json_escape(msg)),
    };
    format!(
        r#"{{"type":"CommandResult","request_id":{},"output":{},"duration_ms":{}}}"#,
        rid, output_json, duration_ms
    )
}

pub fn set_value_ack(rid: u32, path: &str) -> String {
    format!(
        r#"{{"type":"SetValueAck","request_id":{},"path":"{}"}}"#,
        rid,
        json_escape(path)
    )
}

pub fn set_value_error(rid: u32, code: &str, path: &str) -> String {
    format!(
        r#"{{"type":"Error","request_id":{},"code":"{}","message":"{}"}}"#,
        rid,
        json_escape(code),
        json_escape(path)
    )
}

pub fn metric_batch_json(timestamp_ms: u64, interval_ms: u32, samples: &[(String, f64)]) -> String {
    let sample_jsons: Vec<String> = samples
        .iter()
        .map(|(path, value)| format!(r#"{{"path":"{}","value":{}}}"#, path, value))
        .collect();
    format!(
        r#"{{"type":"MetricBatch","timestamp_ms":{},"interval_ms":{},"samples":[{}]}}"#,
        timestamp_ms,
        interval_ms,
        sample_jsons.join(",")
    )
}

pub fn subscribe_metrics_ack_json(
    rid: u32,
    effective_interval_ms: u32,
    active_paths: &[String],
) -> String {
    let path_jsons: Vec<String> = active_paths.iter().map(|p| format!("\"{}\"", p)).collect();
    format!(
        r#"{{"type":"SubscribeMetricsAck","request_id":{},"effective_interval_ms":{},"active_paths":[{}]}}"#,
        rid,
        effective_interval_ms,
        path_jsons.join(",")
    )
}

pub fn unsubscribe_metrics_ack_json(rid: u32, active_paths: &[String]) -> String {
    let path_jsons: Vec<String> = active_paths.iter().map(|p| format!("\"{}\"", p)).collect();
    format!(
        r#"{{"type":"UnsubscribeMetricsAck","request_id":{},"active_paths":[{}]}}"#,
        rid,
        path_jsons.join(",")
    )
}

// ── Tests (run on host via `cargo test --features simulator,debug-inspect`) ─

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_inspect::DebugCommands;

    #[test]
    fn default_state_schema_round_trips() {
        let json = inspect_state_schema_json();
        assert!(json.contains("\"type_name\":\"InspectState\""));
        assert!(json.contains("\"type_name\":\"NetworkState\""));
        assert!(json.contains("\"type_name\":\"BookState\""));
        // Mailbox fields must never appear in the schema.
        assert!(!json.contains("pending_ntp_sync"));
    }

    #[test]
    fn collect_leaf_paths_finds_nested_fields() {
        let state = InspectState::default();
        let mut paths = Vec::new();
        collect_leaf_paths(&state, "", &mut paths);
        assert!(paths.contains(&String::from("network.connected")));
        assert!(paths.contains(&String::from("book.current_filename")));
        assert!(paths.contains(&String::from("settings.utc_offset_minutes")));
    }

    #[test]
    fn get_field_path_reads_nested_value() {
        let mut state = InspectState::default();
        state.book.chapter_idx = 7;
        match state.get_field_path("book.chapter_idx") {
            Some(DebugValue::U32(v)) => assert_eq!(v, 7),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parse_msg_hello() {
        let json = r#"{"type":"Hello","request_id":1}"#;
        match parse_msg(json) {
            InMsg::Hello { request_id } => assert_eq!(request_id, 1),
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn parse_msg_get_value() {
        let json = r#"{"type":"GetValue","request_id":2,"path":"book.chapter_idx"}"#;
        match parse_msg(json) {
            InMsg::GetValue { request_id, path } => {
                assert_eq!(request_id, 2);
                assert_eq!(path, "book.chapter_idx");
            }
            _ => panic!("expected GetValue"),
        }
    }

    #[test]
    fn parse_msg_set_value() {
        let json =
            r#"{"type":"SetValue","request_id":3,"path":"settings.font_size","value":{"kind":"str","value":"Large"}}"#;
        match parse_msg(json) {
            InMsg::SetValue {
                request_id,
                path,
                value_json,
            } => {
                assert_eq!(request_id, 3);
                assert_eq!(path, "settings.font_size");
                match parse_set_value(value_json) {
                    Some(CommandArg::Str(s)) => assert_eq!(s, "Large"),
                    other => panic!("unexpected: {other:?}"),
                }
            }
            _ => panic!("expected SetValue"),
        }
    }

    #[test]
    fn parse_msg_invoke_command_with_args() {
        let json = r#"{"type":"InvokeCommand","request_id":4,"name":"set_font_size","args":[{"kind":"str","value":"Small"}]}"#;
        match parse_msg(json) {
            InMsg::InvokeCommand {
                request_id,
                name,
                args_json,
            } => {
                assert_eq!(request_id, 4);
                assert_eq!(name, "set_font_size");
                let args = parse_command_args(args_json);
                assert_eq!(args.len(), 1);
                match &args[0] {
                    CommandArg::Str(s) => assert_eq!(s, "Small"),
                    other => panic!("unexpected: {other:?}"),
                }
            }
            _ => panic!("expected InvokeCommand"),
        }
    }

    #[test]
    fn parse_msg_malformed_falls_back_to_unknown() {
        assert!(matches!(parse_msg("not json at all"), InMsg::Unknown));
        assert!(matches!(parse_msg(r#"{"type":"Bogus"}"#), InMsg::Unknown));
    }

    #[test]
    fn parse_string_array_round_trip() {
        let arr = parse_string_array(r#"["a.b", "c.d.e"]"#);
        assert_eq!(arr, alloc::vec![String::from("a.b"), String::from("c.d.e")]);
    }

    #[test]
    fn json_escape_handles_quotes_and_control_chars() {
        let escaped = json_escape("hi \"there\"\n\t\\");
        assert_eq!(escaped, "hi \\\"there\\\"\\n\\t\\\\");
    }

    #[test]
    fn slugify_handles_spaces_and_punctuation() {
        assert_eq!(slugify("My E-Reader!"), "my-e-reader");
        assert_eq!(slugify(""), "device");
    }

    #[test]
    fn dispatch_command_reset_counters() {
        let mut state = InspectState::default();
        let result = state.dispatch_command("reset_counters", &[]);
        assert!(result.is_ok());
        assert!(state.pending_reset_counters);
    }

    #[test]
    fn dispatch_command_set_font_size_sets_mailbox() {
        let mut state = InspectState::default();
        let args = [CommandArg::Str(String::from("Large"))];
        let result = state.dispatch_command("set_font_size", &args);
        assert!(result.is_ok());
        assert_eq!(state.pending_font_size, Some(String::from("Large")));
    }

    #[test]
    fn dispatch_command_unknown_name() {
        let mut state = InspectState::default();
        assert!(state.dispatch_command("does_not_exist", &[]).is_err());
    }

    #[test]
    fn commands_response_json_lists_all_commands() {
        let json = commands_response_json(1, InspectState::command_defs());
        assert!(json.contains("\"reset_counters\""));
        assert!(json.contains("\"set_font_size\""));
        assert!(json.contains("\"set_orientation\""));
    }

    #[test]
    fn value_resp_and_changed_resp_shape() {
        assert!(value_resp(1, "book.chapter_idx", "3").contains("\"path\":\"book.chapter_idx\""));
        assert!(changed_resp("network.connected", "true", 9).contains("\"sequence\":9"));
    }
}
