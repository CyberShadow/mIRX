//! The resources `build/` links into every bin (this test harness included):
//! mirx's icon, `VERSIONINFO` and the application manifest's effects.

use std::mem::zeroed;
use std::ptr::null;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Graphics::Gdi::{BITMAP, DeleteObject, GetObjectW};
use windows_sys::Win32::System::LibraryLoader::{
    FindResourceW, GetModuleHandleW, LoadResource, LockResource, SizeofResource,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, ICONINFO, IMAGE_ICON, IsProcessDPIAware, LoadImageW, RT_GROUP_ICON,
    RT_VERSION,
};
use windows_sys::core::PCWSTR;

/// The id `build/resources.rs` gives the icon and the version block.
const RESOURCE_ID: u16 = 1;

fn int_resource(id: u16) -> PCWSTR {
    id as usize as PCWSTR
}

fn this_module() -> HMODULE {
    let module = unsafe { GetModuleHandleW(null()) };
    assert!(!module.is_null(), "GetModuleHandleW(null)");
    module
}

fn resource_bytes(kind: PCWSTR, id: u16) -> &'static [u8] {
    let module = this_module();
    unsafe {
        let info = FindResourceW(module, int_resource(id), kind);
        assert!(
            !info.is_null(),
            "resource {id} of type {kind:?} is linked in"
        );
        let data = LoadResource(module, info);
        assert!(!data.is_null(), "LoadResource");
        let bytes = LockResource(data) as *const u8;
        assert!(!bytes.is_null(), "LockResource");
        std::slice::from_raw_parts(bytes, SizeofResource(module, info) as usize)
    }
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn app_icon_group_has_every_size() {
    // GRPICONDIR: reserved, type, count, then 14-byte GRPICONDIRENTRYs
    // (width, height, colours, reserved, planes, bit count, size, id).
    let group = resource_bytes(RT_GROUP_ICON, RESOURCE_ID);
    assert_eq!(u16_at(group, 2), 1, "an icon group");
    let count = u16_at(group, 4) as usize;
    let entries: Vec<(u8, u16)> = (0..count)
        .map(|index| {
            let entry = 6 + 14 * index;
            (group[entry], u16_at(group, entry + 6))
        })
        .collect();
    assert_eq!(
        entries,
        [16, 20, 24, 32, 40, 48, 64, 0]
            .map(|width| (width, 32))
            .to_vec(),
        "(width, bits) per entry; width 0 is 256"
    );
}

#[test]
fn app_icon_loads_at_small_and_large_metrics() {
    for size in [16, 32, 48, 256] {
        unsafe {
            let icon = LoadImageW(
                this_module(),
                int_resource(RESOURCE_ID),
                IMAGE_ICON,
                size,
                size,
                0,
            );
            assert!(!icon.is_null(), "LoadImageW of the {size} px icon");
            let mut info: ICONINFO = zeroed();
            assert_ne!(GetIconInfo(icon, &mut info), 0, "GetIconInfo {size}");
            let mut bitmap: BITMAP = zeroed();
            assert_ne!(
                GetObjectW(
                    info.hbmColor,
                    size_of::<BITMAP>() as i32,
                    (&mut bitmap as *mut BITMAP).cast()
                ),
                0,
                "GetObjectW of the {size} px colour bitmap"
            );
            assert_eq!((bitmap.bmWidth, bitmap.bmHeight), (size, size));
            DeleteObject(info.hbmColor);
            DeleteObject(info.hbmMask);
            DestroyIcon(icon);
        }
    }
}

#[test]
fn version_info_names_mirx_and_the_crate_version() {
    let version = resource_bytes(RT_VERSION, RESOURCE_ID);
    let fixed = version
        .windows(4)
        .position(|word| word == 0xFEEF_04BDu32.to_le_bytes())
        .expect("VS_FIXEDFILEINFO signature");
    let part = |text: &str| -> u32 { text.parse().unwrap() };
    let major = part(env!("CARGO_PKG_VERSION_MAJOR"));
    let minor = part(env!("CARGO_PKG_VERSION_MINOR"));
    let patch = part(env!("CARGO_PKG_VERSION_PATCH"));
    // dwSignature, dwStrucVersion, dwFileVersionMS, dwFileVersionLS.
    assert_eq!(u32_at(version, fixed + 8), (major << 16) | minor);
    assert_eq!(u32_at(version, fixed + 12), patch << 16);

    let words: Vec<u16> = version
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    let utf16 = |text: &str| -> Vec<u16> { text.encode_utf16().chain([0]).collect() };
    let key = utf16("ProductName");
    let after_key = words
        .windows(key.len())
        .position(|window| window == key)
        .expect("a ProductName string")
        + key.len();
    // The value follows after padding to a 32-bit boundary.
    let value_start = after_key
        + words[after_key..]
            .iter()
            .position(|&word| word != 0)
            .expect("a ProductName value");
    let value = utf16("mirx");
    assert_eq!(&words[value_start..value_start + value.len()], value);
}

#[test]
fn process_is_dpi_unaware() {
    // The manifest says so: the layout is in 96-dpi pixels until DPI scaling
    // lands, and Windows bitmap-stretches the windows meanwhile.
    assert_eq!(unsafe { IsProcessDPIAware() }, 0);
}
