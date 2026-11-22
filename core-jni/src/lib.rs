//! The JNI bridge for the Android app (`protocol/RustShadow.kt`). Shadow mode: the app feeds the OPPO driver the
//! same bytes the Kotlin layer reads and compares the two states. The driver's frames go nowhere.
//! Each call catches a panic, so a core bug shows as a log line, never as a crash.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use buds_core::model::Wear;
use buds_core::oppo::driver::{Driver, Settings};
use jni::objects::{JByteArray, JObject, JString};
use jni::sys::{jboolean, jlong, jstring};
use jni::JNIEnv;
use serde_json::json;

fn with<R>(h: jlong, f: impl FnOnce(&mut Driver) -> R) -> Option<R> {
    if h == 0 { return None; }
    // SAFETY: `h` comes from `nativeNew` and the app never frees it.
    let m = unsafe { &*(h as *const Mutex<Driver>) };
    catch_unwind(AssertUnwindSafe(|| f(&mut m.lock().unwrap_or_else(|e| e.into_inner())))).ok()
}

fn text(env: &mut JNIEnv, s: &JString) -> Option<String> {
    if s.is_null() { return None; }
    env.get_string(s).ok().map(Into::into)
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeNew(_env: JNIEnv, _this: JObject) -> jlong {
    Box::into_raw(Box::new(Mutex::new(Driver::new(Settings::default())))) as jlong
}

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeConnected(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong, name: JString, product: JString, manual: JString,
    clear_battery: jboolean, phone_spatial: jboolean,
) {
    let settings = Settings {
        product_id: text(&mut env, &product),
        model_manual: text(&mut env, &manual),
        clear_battery: clear_battery != 0,
        phone_spatial: phone_spatial != 0,
    };
    let name = text(&mut env, &name).unwrap_or_default();
    with(h, |d| { d.set_settings(settings); d.connected(now as u64, name); });
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeBytes(
    env: JNIEnv, _this: JObject, h: jlong, now: jlong, unix: jlong, data: JByteArray,
) {
    let Ok(bytes) = env.convert_byte_array(&data) else { return };
    with(h, |d| { d.on_bytes(now as u64, unix as u32, &bytes); });
}

#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeDisconnected(
    _env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) {
    with(h, |d| { d.disconnected(now as u64); });
}

/// The shared state as JSON: `connected`, `battery` (left, right, case; -1 unknown), `wear` (`ear`, `out`,
/// `case`, `?`), `noise`, `lowLatency`, `features`. Null when the core failed.
#[no_mangle]
pub extern "system" fn Java_com_spizganed_quickbuds_protocol_RustShadow_nativeCommon(
    mut env: JNIEnv, _this: JObject, h: jlong, now: jlong,
) -> jstring {
    let state = with(h, |d| {
        d.tick(now as u64);
        let c = d.common();
        let wear = |w: Wear| match w { Wear::InEar => "ear", Wear::Out => "out", Wear::InCase => "case", Wear::Unknown => "?" };
        json!({
            "connected": c.connected,
            "battery": c.battery.map(|b| b.map_or(-1, |l| l.value as i32)),
            "wear": c.wear.map(wear),
            "noise": c.noise,
            "lowLatency": c.low_latency,
            "features": c.features,
        }).to_string()
    });
    match state.and_then(|s| env.new_string(s).ok()) {
        Some(s) => s.into_raw(),
        None => std::ptr::null_mut(),
    }
}
