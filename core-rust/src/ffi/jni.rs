//! The minimal subset of the JNI C ABI that Pixel Studio needs.
//!
//! `JNIEnv*` points to a pointer to the JNINativeInterface function table. Function slots
//! are fixed by the JNI specification ("JNI Functions" chapter, interface function table),
//! which has been append-only since JNI 1.1.

#![allow(non_camel_case_types)]

use std::ffi::c_void;

pub type jint = i32;
pub type jlong = i64;
pub type jsize = jint;
pub type jbyte = i8;
pub type jobject = *mut c_void;
pub type jarray = jobject;
pub type jbyteArray = jarray;

/// Opaque; only ever used behind a pointer.
#[repr(C)]
pub struct JNIEnv {
    functions: *const *const c_void,
}

const GET_ARRAY_LENGTH: usize = 171;
const NEW_BYTE_ARRAY: usize = 176;
const GET_BYTE_ARRAY_REGION: usize = 200;
const SET_BYTE_ARRAY_REGION: usize = 208;
const EXCEPTION_CHECK: usize = 228;

type GetArrayLength = unsafe extern "system" fn(*mut JNIEnv, jarray) -> jsize;
type NewByteArray = unsafe extern "system" fn(*mut JNIEnv, jsize) -> jbyteArray;
type GetByteArrayRegion = unsafe extern "system" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *mut jbyte);
type SetByteArrayRegion = unsafe extern "system" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *const jbyte);
type ExceptionCheck = unsafe extern "system" fn(*mut JNIEnv) -> u8;

/// Looks up a function slot in the table.
///
/// # Safety
/// `env` must be a valid JNIEnv pointer for the current thread and `F` must be the
/// exact function type of `slot`.
unsafe fn slot<F: Copy>(env: *mut JNIEnv, slot: usize) -> F {
    unsafe {
        let table = (*env).functions;
        let f: *const c_void = *table.add(slot);
        std::mem::transmute_copy::<*const c_void, F>(&f)
    }
}

/// Copies a Java `byte[]` into a Rust vector. `None` for a null array.
///
/// # Safety
/// `env` must be valid for this thread and `array` a live local/global reference or null.
pub unsafe fn read_byte_array(env: *mut JNIEnv, array: jbyteArray) -> Option<Vec<u8>> {
    if env.is_null() || array.is_null() {
        return None;
    }
    unsafe {
        let len = slot::<GetArrayLength>(env, GET_ARRAY_LENGTH)(env, array);
        if len <= 0 {
            return Some(Vec::new());
        }
        let mut buf = vec![0u8; len as usize];
        slot::<GetByteArrayRegion>(env, GET_BYTE_ARRAY_REGION)(env, array, 0, len, buf.as_mut_ptr() as *mut jbyte);
        if slot::<ExceptionCheck>(env, EXCEPTION_CHECK)(env) != 0 {
            return None;
        }
        Some(buf)
    }
}

/// Allocates a Java `byte[]` holding `data`. Returns null (with a pending Java exception,
/// e.g. OutOfMemoryError) if allocation fails.
///
/// # Safety
/// `env` must be valid for this thread.
pub unsafe fn new_byte_array(env: *mut JNIEnv, data: &[u8]) -> jbyteArray {
    if env.is_null() || data.len() > i32::MAX as usize {
        return std::ptr::null_mut();
    }
    unsafe {
        let array = slot::<NewByteArray>(env, NEW_BYTE_ARRAY)(env, data.len() as jsize);
        if array.is_null() {
            return array;
        }
        slot::<SetByteArrayRegion>(env, SET_BYTE_ARRAY_REGION)(env, array, 0, data.len() as jsize, data.as_ptr() as *const jbyte);
        array
    }
}
