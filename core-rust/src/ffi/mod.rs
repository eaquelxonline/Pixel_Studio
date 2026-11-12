//! Hand-written JNI bridge (no `jni` crate).
//!
//! Kotlin side: `com.pixel.studio.bridge.NativeBridge`
//! ```kotlin
//! external fun abiVersion(): Int
//! external fun create(width: Int, height: Int): Long
//! external fun destroy(handle: Long)
//! external fun revision(handle: Long): Long
//! external fun call(handle: Long, request: ByteArray, payload: ByteArray?): ByteArray
//! ```
//! Safety rules:
//! * Handles are opaque registry ids, never raw pointers: a stale or forged handle is
//!   reported as an error instead of dereferencing freed memory.
//! * Every entry point catches panics; unwinding never crosses into the JVM.
//! * Only four JNIEnv functions are used, all with bounds-checked lengths.
//! * Engines are guarded by a mutex: one writer at a time (the single-writer queue).
//!
//! `call` responses are framed: `u32 LE json length | UTF-8 JSON | binary payload`.

// JNI entry points receive raw pointers from the JVM by definition.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod jni;

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, OnceLock};

use crate::engine::Engine;
use jni::{JNIEnv, jbyteArray, jint, jlong, jobject};

/// Bumped whenever the native interface changes shape.
pub const ABI_VERSION: i32 = 1;

type Registry = Mutex<HashMap<u64, Arc<Mutex<Engine>>>>;

fn registry() -> &'static Registry {
    static REG: OnceLock<Registry> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

static NEXT_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn create_engine(width: u32, height: u32) -> u64 {
    let handle = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut reg = registry().lock().unwrap_or_else(|p| p.into_inner());
    reg.insert(handle, Arc::new(Mutex::new(Engine::new(width, height))));
    handle
}

pub fn destroy_engine(handle: u64) -> bool {
    registry().lock().unwrap_or_else(|p| p.into_inner()).remove(&handle).is_some()
}

pub fn with_engine<R>(handle: u64, f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
    let engine = registry().lock().unwrap_or_else(|p| p.into_inner()).get(&handle).cloned()?;
    let mut guard = engine.lock().unwrap_or_else(|p| p.into_inner());
    Some(f(&mut guard))
}

pub fn frame_response(json: &str, binary: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + json.len() + binary.len());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    out.extend_from_slice(binary);
    out
}

fn error_frame(code: &str, message: &str) -> Vec<u8> {
    let json = crate::commands::json::Json::obj()
        .with("ok", false)
        .with("error", crate::commands::json::Json::obj().with("code", code).with("message", message))
        .to_json_string();
    frame_response(&json, &[])
}

/// Executes a request against an engine handle and returns the framed response.
pub fn call(handle: u64, request: &[u8], payload: &[u8]) -> Vec<u8> {
    let result = catch_unwind(AssertUnwindSafe(|| with_engine(handle, |e| e.execute(request, payload))));
    match result {
        Ok(Some(r)) => frame_response(&r.json, &r.binary),
        Ok(None) => error_frame("invalid_handle", "engine handle is not valid (destroyed or never created)"),
        Err(_) => error_frame("internal", "engine panicked; the operation was not applied"),
    }
}

// ---- JNI exports -------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_pixel_studio_bridge_NativeBridge_abiVersion(_env: *mut JNIEnv, _this: jobject) -> jint {
    ABI_VERSION
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_pixel_studio_bridge_NativeBridge_create(
    _env: *mut JNIEnv,
    _this: jobject,
    width: jint,
    height: jint,
) -> jlong {
    catch_unwind(|| match crate::project::validate_size(width as i64, height as i64) {
        Ok((w, h)) => create_engine(w, h) as jlong,
        Err(_) => 0,
    })
    .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_pixel_studio_bridge_NativeBridge_destroy(_env: *mut JNIEnv, _this: jobject, handle: jlong) {
    let _ = catch_unwind(|| destroy_engine(handle as u64));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_pixel_studio_bridge_NativeBridge_revision(_env: *mut JNIEnv, _this: jobject, handle: jlong) -> jlong {
    catch_unwind(|| with_engine(handle as u64, |e| e.revision() as jlong).unwrap_or(-1)).unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_pixel_studio_bridge_NativeBridge_call(
    env: *mut JNIEnv,
    _this: jobject,
    handle: jlong,
    request: jbyteArray,
    payload: jbyteArray,
) -> jbyteArray {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: `env` is the JNIEnv pointer the JVM passed to this native method and
        // the arrays are local references valid for the duration of this call.
        unsafe {
            let req = jni::read_byte_array(env, request).unwrap_or_default();
            let pay = jni::read_byte_array(env, payload).unwrap_or_default();
            let response = call(handle as u64, &req, &pay);
            jni::new_byte_array(env, &response)
        }
    }));
    outcome.unwrap_or(std::ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_are_validated() {
        let h = create_engine(8, 8);
        let ok = call(h, br#"{"op":"status"}"#, &[]);
        let len = u32::from_le_bytes(ok[..4].try_into().unwrap()) as usize;
        assert!(std::str::from_utf8(&ok[4..4 + len]).unwrap().contains("\"ok\":true"));
        assert!(destroy_engine(h));
        assert!(!destroy_engine(h));
        let gone = call(h, br#"{"op":"status"}"#, &[]);
        assert!(String::from_utf8_lossy(&gone).contains("invalid_handle"));
        assert!(String::from_utf8_lossy(&call(0xDEAD_BEEF, b"{}", &[])).contains("invalid_handle"));
    }
}
