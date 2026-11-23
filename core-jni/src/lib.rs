//! The JNI bridge for the Android app (`protocol/RustShadow.kt`). Shadow mode: the app feeds the OPPO driver the
//! same bytes the Kotlin layer reads and compares the two states. The driver's frames go nowhere; the bridge keeps
//! the commands of its init sequence, so the app can compare them with its own.
//! Each call catches a panic, so a core bug shows as a log line, never as a crash.

// The state JSON is one large `json!` literal.
#![recursion_limit = "256"]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use buds_core::model::Wear;
use buds_core::oppo::driver::{Cmd, Driver, Effect, EqSaved, Settings, Status};
use buds_core::types::{HearingEv, Preset};
use buds_core::types::{cmd_of, payload_of};
use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jboolean, jint, jlong, jstring};
use jni::JNIEnv;
use serde_json::{json, Map, Value};

struct Shadow {
    driver: Driver,
    /// The commands the driver would send from the connect to the end of its init sequence.
    init_sent: Vec<u16>,
    ready: bool,
}

impl Shadow {
    fn take(&mut self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Send(p) if !self.ready => self.init_sent.extend(commands(&p)),
                Effect::Ready => self.ready = true,
                _ => {}
            }
        }
    }
}

/// The command of a frame, or the commands inside a `0x012F` batch: `<count>`, then `<cmd LE> <len LE> <payload>`.
fn commands(p: &[u8]) -> Vec<u16> {
    let cmd = cmd_of(p);
    if cmd != 0x012F { return vec![cmd]; }
    let pl = payload_of(p);
    let mut out = Vec::new();
    let mut i = 1;
    while i + 4 <= pl.len() {
        out.push(u16::from_le_bytes([pl[i], pl[i + 1]]));
        i += 4 + u16::from_le_bytes([pl[i + 2], pl[i + 3]]) as usize;
    }
    out
}

fn with<R>(h: jlong, f: impl FnOnce(&mut Shadow) -> R) -> Option<R> {
    if h == 0 { return None; }
    // SAFETY: `h` comes from `nativeNew` and the app never frees it.
    let m = unsafe { &*(h as *const Mutex<Shadow>) };
    catch_unwind(AssertUnwindSafe(|| f(&mut m.lock().unwrap_or_else(|e| e.into_inner())))).ok()
}

fn text(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() { return None; }
    env.get_string(s).ok().map(Into::into)
}

fn out(env: &mut JNIEnv, s: Option<String>) -> jstring {
    match s.and_then(|s| env.new_string(s).ok()) {
        Some(s) => s.into_raw(),
        None => std::ptr::null_mut(),
    }
}

/// One preset as JSON: `id`, `name`, `freqs`, `gains`, `selected`, `tag`.
fn preset(p: &Value) -> Preset {
    let nums = |x: &Value| x.as_array().map(|a| a.iter().filter_map(Value::as_i64).collect::<Vec<_>>()).unwrap_or_default();
    let tag = nums(&p["tag"]);
    Preset {
        id: p["id"].as_u64().unwrap_or(0) as u8,
        name: p["name"].as_str().unwrap_or("").into(),
        freqs: nums(&p["freqs"]).into_iter().map(|f| f as u16).collect(),
        gains: nums(&p["gains"]).into_iter().map(|g| g as i8).collect(),
        selected: p["selected"].as_bool().unwrap_or(false),
        tag: [tag.first().copied().unwrap_or(0) as u8, tag.get(1).copied().unwrap_or(0) as u8],
    }
}

/// The app's EQ state as JSON: `current`, `bass`, `custom` (`id`, `name`, `freqs`, `gains`, `selected`, `tag`).
fn eq_saved(text: &str) -> Option<EqSaved> {
    let v: Value = serde_json::from_str(text).ok()?;
    let nums = |x: &Value| x.as_array().map(|a| a.iter().filter_map(Value::as_i64).collect::<Vec<_>>()).unwrap_or_default();
    let custom = v["custom"].as_array()?.iter().map(|p| {
        let tag = nums(&p["tag"]);
        Preset {
            id: p["id"].as_u64().unwrap_or(0) as u8,
            name: p["name"].as_str().unwrap_or("").into(),
            freqs: nums(&p["freqs"]).into_iter().map(|f| f as u16).collect(),
            gains: nums(&p["gains"]).into_iter().map(|g| g as i8).collect(),
            selected: p["selected"].as_bool().unwrap_or(false),
            tag: [tag.first().copied().unwrap_or(0) as u8, tag.get(1).copied().unwrap_or(0) as u8],
        }
    }).collect();
    Some(EqSaved { current: v["current"].as_u64().map(|c| c as u8), bass_level: v["bass"].as_i64().map(|b| b as i8), custom })
}

fn mac(m: &[u8; 6]) -> String { m.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":") }

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeNew(_env: JNIEnv, _this: JObject) -> jlong {
    let s = Shadow { driver: Driver::new(Settings::default()), init_sent: Vec::new(), ready: false };
    Box::into_raw(Box::new(Mutex::new(s))) as jlong
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeConnected(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong, name: JString, product: JString, manual: JString,
    clear_battery: jboolean, phone_spatial: jboolean, case_level: jint, eq: JString,
) {
    let settings = Settings {
        product_id: text(&mut env, &product),
        model_manual: text(&mut env, &manual),
        clear_battery: clear_battery != 0,
        phone_spatial: phone_spatial != 0,
        case_level: u8::try_from(case_level).ok().filter(|&l| l > 0),
        // The app reads the EQ when its screen opens.
        eq_on_connect: false,
        eq_saved: text(&mut env, &eq).and_then(|t| eq_saved(&t)),
    };
    let name = text(&mut env, &name).unwrap_or_default();
    with(h, |s| {
        s.init_sent.clear();
        s.ready = false;
        s.driver.set_settings(settings);
        let e = s.driver.connected(now as u64, name);
        s.take(e);
    });
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeBytes(
    env: JNIEnv, _this: JObject, h: jlong, now: jlong, unix: jlong, data: JByteArray,
) {
    let Ok(bytes) = env.convert_byte_array(&data) else { return };
    with(h, |s| { let e = s.driver.on_bytes(now as u64, unix as u32, &bytes); s.take(e); });
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeDisconnected(
    _env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) {
    with(h, |s| { let e = s.driver.disconnected(now as u64); s.take(e); });
}

/// A case level the app read from the BLE advertisement.
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeCaseStandIn(
    _env: JNIEnv, _this: JObject, h: jlong, now: jlong, level: jint, charging: jboolean,
) {
    let Ok(level) = u8::try_from(level) else { return };
    with(h, |s| { let e = s.driver.case_stand_in(now as u64, level, charging != 0); s.take(e); });
}

/// The clock: runs the steps that are due. Returns the time of the next call, or -1 for none.
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeTick(
    _env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) -> jlong {
    with(h, |s| {
        let e = s.driver.tick(now as u64);
        s.take(e);
        s.driver.next_wake().map_or(-1, |t| t as jlong)
    }).unwrap_or(-1)
}

/// The commands of the driver's init sequence, as hex ids (`0100,0103,…`), and `+` at the end once it is done.
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeInitSent(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) -> jstring {
    let t = with(h, |s| {
        let e = s.driver.tick(now as u64);
        s.take(e);
        let ids: Vec<String> = s.init_sent.iter().map(|c| format!("{c:04X}")).collect();
        format!("{}{}", ids.join(","), if s.ready { "+" } else { "" })
    });
    out(&mut env, t)
}

/// The state as JSON. Shared state (`core/src/model.rs`): `connected`, `battery` (left, right, case; -1 unknown),
/// `wear` (`ear`, `out`, `case`, `?`), `noise`, `lowLatency`, `features`. OPPO state the Kotlin layer keeps: `eq`,
/// `eqCustom` (`id:name:gains`), `bass`, `switches` (`0x810D` values by id), `tap` (level, default), `alert`,
/// `devices` (`mac|name|connected|this`), `preferred`, `firmware`, `spatialType`, `gameSound`, `gameSounds`,
/// `smart`, `head`, `codec`, `codecs`, `holdMasks` (type to mask), `keyfn` (`dev:button:act:fn`), `fit`
/// (left, right), `pnc` (kind, value), `busy`. Null when the core failed.
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeCommon(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) -> jstring {
    let state = with(h, |sh| {
        let e = sh.driver.tick(now as u64);
        sh.take(e);
        state_json(&sh.driver)
    });
    out(&mut env, state)
}

/// The driver's state as JSON (see `nativeCommon`).
fn state_json(d: &Driver) -> String {
    let c = d.common();
    let s = d.snapshot();
    let wear = |w: Wear| match w { Wear::InEar => "ear", Wear::Out => "out", Wear::InCase => "case", Wear::Unknown => "?" };
    let custom: Vec<String> = s.eq_custom.iter().map(|p| format!("{}:{}:{}", p.id, p.name,
        p.gains.iter().map(|g| g.to_string()).collect::<Vec<_>>().join(","))).collect();
    let switches: Map<String, Value> = s.features.iter().map(|&(id, v)| (id.to_string(), v.into())).collect();
    let masks: Map<String, Value> = s.hold_masks.iter().map(|&(t, m)| (t.to_string(), m.into())).collect();
    let devices: Vec<String> = s.devices.iter()
        .map(|d| format!("{}|{}|{}|{}", mac(&d.mac), d.name, d.connected, d.this_device)).collect();
    let keyfn: Option<Vec<String>> = s.keyfn.as_ref()
        .map(|t| t.iter().map(|e| format!("{}:{}:{}:{}", e[0], e[1], e[2], e[3])).collect());
    json!({
        "connected": c.connected,
        "battery": c.battery.map(|b| b.map_or(-1, |l| l.value as i32)),
        "wear": c.wear.map(wear),
        "noise": c.noise,
        "lowLatency": c.low_latency,
        "features": c.features,
        "eq": s.eq_current,
        "eqCustom": custom,
        "bass": s.bass_level,
        "switches": switches,
        "tap": s.tap_level.map(|(l, d)| [l, d]),
        "alert": s.alert_volume,
        "devices": devices,
        "preferred": s.preferred.as_ref().map(mac),
        "firmware": s.firmware,
        "spatialType": s.spatial_type,
        "gameSound": s.game_sound.as_ref().map(|g| g.0),
        "gameSounds": s.game_sound.as_ref().map(|g| g.1.clone()).unwrap_or_default(),
        "smart": s.smart_level,
        "head": s.head_motion,
        "codec": s.codec,
        "codecs": s.codecs,
        "holdMasks": masks,
        "keyfn": keyfn,
        "fit": (s.fit.2 > 0).then_some([s.fit.0, s.fit.1]),
        "pnc": (s.pnc.2 > 0).then_some([s.pnc.0, s.pnc.1]),
        "busy": s.busy,
        // Driver mode only: what the Kotlin listeners need beyond the shadow compare.
        "status": match s.status { Status::Off => 0, Status::Connecting => 1, Status::On => 2 },
        "charging": s.battery.map(|b| b.is_some_and(|x| x.1)),
        "wearRaw": s.wear.map(|w| w.map_or(-1, i32::from)),
        "anc": s.anc,
        "ancRefused": s.anc_refused,
        "offered": s.offered,
        "caps": s.caps.0,
        "model": s.model.and_then(|m| m["id"].as_str()),
        "bassOn": s.bass_on,
        "bassBands": s.bass_bands,
        "eqPresets": s.eq_custom.iter().map(|p| json!({ "id": p.id, "name": p.name, "freqs": p.freqs, "gains": p.gains,
            "selected": p.selected, "tag": p.tag })).collect::<Vec<_>>(),
        "notice": s.notice.map(|n| json!({ "key": n.key, "kind": n.kind.name(), "seq": n.seq })),
    }).to_string()
}

// ---- Driver mode (`protocol/RustDriver.kt`): the core drives the link. ----

fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02X}")).collect() }

fn unhex(t: &str) -> Vec<u8> {
    let t: Vec<u8> = t.bytes().filter(u8::is_ascii_hexdigit).collect();
    t.chunks_exact(2).filter_map(|c| u8::from_str_radix(std::str::from_utf8(c).ok()?, 16).ok()).collect()
}

fn mac_bytes(t: &str) -> Option<[u8; 6]> { unhex(t).try_into().ok() }

fn floats(v: &[f32]) -> Vec<Value> { v.iter().map(|&f| json!(f)).collect() }

/// The effects as a JSON array, in order. `send` and `log` carry hex bytes.
fn effects_json(effects: Vec<Effect>) -> String {
    let list: Vec<Value> = effects.into_iter().map(|e| match e {
        Effect::Send(p) => json!({ "send": hex(&p) }),
        Effect::Log { dir, bytes, human } => json!({ "log": dir, "hex": hex(&bytes), "human": human }),
        Effect::State => json!({ "state": true }),
        Effect::Save(k, v) => json!({ "save": k, "value": v }),
        Effect::Ready => json!({ "ready": true }),
        Effect::FindPhone(on) => json!({ "findPhone": on }),
        Effect::Hearing(h) => json!({ "hearing": match h {
            HearingEv::Status(kind, st) => json!({ "t": "status", "kind": kind, "status": st }),
            HearingEv::Refused(st) => json!({ "t": "refused", "status": st }),
            HearingEv::EarScan(uid, d) => json!({ "t": "earScan", "uid": uid, "hex": hex(&d) }),
            HearingEv::Filter(uid, en, c) => json!({ "t": "filter", "uid": uid, "enhance": en,
                "curves": c.map(|(fs, l, r)| json!({ "fs": fs, "left": floats(&l), "right": floats(&r) })) }),
            HearingEv::ScanCurves(uid, (fs, l, r)) => json!({ "t": "scanCurves", "uid": uid, "fs": fs, "left": floats(&l), "right": floats(&r) }),
            HearingEv::Active(uid, name, v) => json!({ "t": "active", "uid": uid, "name": name, "values": v }),
            HearingEv::ActiveScan(uid, d) => json!({ "t": "activeScan", "uid": uid, "hex": hex(&d) }),
        } }),
    }).collect();
    Value::Array(list).to_string()
}

/// A UI command from JSON: `{"c": <name>, ...}`. None for an unknown name or a missing field.
fn command(v: &Value) -> Option<Cmd> {
    let u = |k: &str| v[k].as_u64().map(|x| x as u8);
    let b = |k: &str| v[k].as_bool();
    Some(match v["c"].as_str()? {
        "anc" => Cmd::Anc(v["mode"].as_str()?.into()),
        "lowLatency" => Cmd::LowLatency(b("on")?),
        "eqBuiltIn" => Cmd::EqBuiltIn(u("id")?),
        "eqSave" => Cmd::EqSave(preset(&v["preset"])),
        "eqCreate" => Cmd::EqCreate(preset(&v["preset"])),
        "eqDelete" => Cmd::EqDelete(preset(&v["preset"])),
        "bassWave" => Cmd::BassWave(b("on")?),
        "bassLevel" => Cmd::BassLevel(v["level"].as_i64()? as i8, u("band")?),
        "features" => Cmd::Features(v["list"].as_array()?.iter()
            .filter_map(|x| Some((x[0].as_u64()? as u8, x[1].as_bool()?))).collect()),
        "find" => Cmd::Find(b("on")?),
        "modelPicked" => Cmd::ModelPicked,
        "earbudReads" => Cmd::EarbudReads,
        "alertVolume" => Cmd::AlertVolume(u("level")?),
        "tapLevel" => Cmd::TapLevel(u("level")?),
        "gameSoundType" => Cmd::GameSoundType(u("type")?),
        "gameEq" => Cmd::GameEq(b("on")?),
        "spatialType" => Cmd::SpatialType(u("type")?),
        "codec" => Cmd::Codec(u("codec")?, b("hires")?),
        "hires" => Cmd::Hires { on: b("on")?, drop_spatial: b("dropSpatial")? },
        "headMotion" => Cmd::HeadMotion(u("type")?),
        "fitTest" => Cmd::FitTest(b("on")?),
        "pncQuery" => Cmd::PncQuery,
        "pnc" => Cmd::Pnc(u("action")?),
        "statusRead" => Cmd::StatusRead,
        "dual" => Cmd::Dual(b("on")?),
        "dualReads" => Cmd::DualReads(b("preferred")?),
        "connectDevice" => Cmd::ConnectDevice(mac_bytes(v["mac"].as_str()?)?, b("on")?),
        "preferred" => Cmd::Preferred(match v["mac"].as_str() { Some(m) => Some(mac_bytes(m)?), None => None }),
        "controlsReads" => Cmd::ControlsReads(v["types"].as_array()?.iter().filter_map(|t| t.as_u64().map(|t| t as u8)).collect()),
        "gesture" => Cmd::Gesture(u("dev")?, u("act")?, u("fn")?),
        "holdModes" => Cmd::HoldModes(v["mask"].as_u64()? as u32, u("type")?),
        "onCall" => Cmd::OnCall(u("button")?, u("act")?, u("fn")?),
        "hearing" => Cmd::Hearing(v["frames"].as_array()?.iter()
            .filter_map(|f| Some((f[0].as_u64()? as u16, unhex(f[1].as_str()?)))).collect()),
        "batteryRead" => Cmd::BatteryRead,
        _ => return None,
    })
}

fn driver<R>(h: jlong, f: impl FnOnce(&mut Driver) -> R) -> Option<R> {
    if h == 0 { return None; }
    // SAFETY: `h` comes from `RustDriver.nativeNew` and the app never frees it.
    let m = unsafe { &*(h as *const Mutex<Driver>) };
    catch_unwind(AssertUnwindSafe(|| f(&mut m.lock().unwrap_or_else(|e| e.into_inner())))).ok()
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeNew(_env: JNIEnv, _this: JObject) -> jlong {
    Box::into_raw(Box::new(Mutex::new(Driver::new(Settings::default())))) as jlong
}

/// The settings the driver reads; call before `nativeConnected` and after a change.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeSettings(
    mut env: JNIEnv, _this: JObject, h: jlong, product: JString, manual: JString,
    clear_battery: jboolean, phone_spatial: jboolean, case_level: jint, eq: JString,
) {
    let settings = Settings {
        product_id: text(&mut env, &product),
        model_manual: text(&mut env, &manual),
        clear_battery: clear_battery != 0,
        phone_spatial: phone_spatial != 0,
        case_level: u8::try_from(case_level).ok().filter(|&l| l > 0),
        eq_on_connect: false,
        eq_saved: text(&mut env, &eq).and_then(|t| eq_saved(&t)),
    };
    driver(h, |d| d.set_settings(settings));
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeConnected(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong, name: JString,
) -> jstring {
    let name = text(&mut env, &name).unwrap_or_default();
    let e = driver(h, |d| effects_json(d.connected(now as u64, name)));
    out(&mut env, e)
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeBytes(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong, unix: jlong, data: JByteArray,
) -> jstring {
    let Ok(bytes) = env.convert_byte_array(&data) else { return std::ptr::null_mut() };
    let e = driver(h, |d| effects_json(d.on_bytes(now as u64, unix as u32, &bytes)));
    out(&mut env, e)
}

/// A UI command as JSON (see `command`). An unknown command returns null.
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeCommand(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong, json: JString,
) -> jstring {
    let c = text(&mut env, &json).and_then(|t| serde_json::from_str::<Value>(&t).ok()).as_ref().and_then(command);
    let Some(c) = c else { return std::ptr::null_mut() };
    let e = driver(h, |d| effects_json(d.command(now as u64, c)));
    out(&mut env, e)
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeTick(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) -> jstring {
    let e = driver(h, |d| effects_json(d.tick(now as u64)));
    out(&mut env, e)
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeDisconnected(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) -> jstring {
    let e = driver(h, |d| effects_json(d.disconnected(now as u64)));
    out(&mut env, e)
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeCaseStandIn(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong, level: jint, charging: jboolean,
) -> jstring {
    let Ok(level) = u8::try_from(level) else { return std::ptr::null_mut() };
    let e = driver(h, |d| effects_json(d.case_stand_in(now as u64, level, charging != 0)));
    out(&mut env, e)
}

/// The time of the next `nativeTick`, or -1 for none.
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeNextWake(_env: JNIEnv, _this: JObject, h: jlong) -> jlong {
    driver(h, |d| d.next_wake().map_or(-1, |t| t as jlong)).unwrap_or(-1)
}

/// The state as JSON (`state_json`).
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustDriver_nativeState(mut env: JNIEnv, _this: JObject, h: jlong) -> jstring {
    let st = driver(h, |d| state_json(d));
    out(&mut env, st)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_command_shape() {
        let ok = |t: &str| command(&serde_json::from_str(t).unwrap()).is_some();
        assert!(ok(r#"{"c":"anc","mode":"ANC-Max"}"#));
        assert!(ok(r#"{"c":"features","list":[[17,true],[12,false]]}"#));
        assert!(ok(r#"{"c":"preferred","mac":null}"#));
        assert!(ok(r#"{"c":"connectDevice","mac":"AA:BB:CC:DD:EE:FF","on":true}"#));
        assert!(ok(r#"{"c":"hearing","frames":[[1037,"0102"]]}"#));
        assert!(ok(r#"{"c":"eqSave","preset":{"id":1,"name":"A","freqs":[100],"gains":[2],"tag":[250,6]}}"#));
        assert!(!ok(r#"{"c":"anc"}"#));
        assert!(!ok(r#"{"c":"nope"}"#));
    }

    #[test]
    fn effects_carry_hex_frames() {
        let t = effects_json(vec![Effect::Send(vec![0xAA, 0x07]), Effect::Ready]);
        assert_eq!(t, r#"[{"send":"AA07"},{"ready":true}]"#);
    }
}
