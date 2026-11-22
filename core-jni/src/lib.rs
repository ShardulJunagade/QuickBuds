//! The JNI bridge for the Android app (`protocol/RustShadow.kt`). Shadow mode: the app feeds the OPPO driver the
//! same bytes the Kotlin layer reads and compares the two states. The driver's frames go nowhere; the bridge keeps
//! the commands of its init sequence, so the app can compare them with its own.
//! Each call catches a panic, so a core bug shows as a log line, never as a crash.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use buds_core::model::Wear;
use buds_core::oppo::driver::{Driver, Effect, EqSaved, Settings};
use buds_core::types::Preset;
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
        let c = sh.driver.common();
        let s = sh.driver.snapshot();
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
        }).to_string()
    });
    out(&mut env, state)
}
