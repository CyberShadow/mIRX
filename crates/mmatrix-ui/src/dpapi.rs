use std::iter::once;
use std::ptr::{null, null_mut};
use std::slice;

use windows_sys::Win32::Foundation::{GetLastError, LocalFree};
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, CRYPT_INTEGER_BLOB,
    CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};

pub(crate) fn protect(data: &[u8], description: &str, entropy: &[u8]) -> Result<Vec<u8>, String> {
    let mut input = blob(data);
    let mut optional_entropy = blob(entropy);
    let mut output = CRYPT_INTEGER_BLOB::default();
    let description: Vec<u16> = description.encode_utf16().chain(once(0)).collect();

    let protected = unsafe {
        CryptProtectData(
            &mut input,
            description.as_ptr(),
            blob_ptr(entropy, &mut optional_entropy),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if protected == 0 {
        return Err(win32_error("CryptProtectData"));
    }

    copy_blob_bytes_and_free(output)
}

pub(crate) fn unprotect(data: &[u8], entropy: &[u8]) -> Result<Vec<u8>, String> {
    let mut input = blob(data);
    let mut optional_entropy = blob(entropy);
    let mut output = CRYPT_INTEGER_BLOB::default();

    let unprotected = unsafe {
        CryptUnprotectData(
            &mut input,
            null_mut(),
            blob_ptr(entropy, &mut optional_entropy),
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if unprotected == 0 {
        return Err(win32_error("CryptUnprotectData"));
    }

    copy_blob_bytes_and_free(output)
}

pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    let status = unsafe {
        BCryptGenRandom(
            null_mut(),
            bytes.as_mut_ptr(),
            N as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(format!(
            "BCryptGenRandom failed with NTSTATUS 0x{status:08x}"
        ));
    }

    Ok(bytes)
}

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

fn blob_ptr<'a>(data: &[u8], blob: &'a mut CRYPT_INTEGER_BLOB) -> *mut CRYPT_INTEGER_BLOB {
    if data.is_empty() { null_mut() } else { blob }
}

fn copy_blob_bytes_and_free(blob: CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, String> {
    let bytes = unsafe { slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec() };
    unsafe {
        if !LocalFree(blob.pbData.cast()).is_null() {
            return Err(win32_error("LocalFree"));
        }
    }
    Ok(bytes)
}

fn win32_error(operation: &str) -> String {
    format!("{operation} failed with Win32 error {}", unsafe {
        GetLastError()
    })
}
