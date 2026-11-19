//! The state each of mirx's windows keeps behind `GWLP_USERDATA`.
//!
//! The pointer is always to a [`WindowData`], which starts with the type of
//! the state it holds, and every lookup names the type it expects: asking a
//! dialog for the frame's state panics instead of reading the dialog's state
//! as the frame's. Only windows of mirx's own classes are asked; another
//! window's `GWLP_USERDATA` is not a `WindowData`.

use std::any::TypeId;
use std::ffi::c_void;
use std::ops::{Deref, DerefMut};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CREATESTRUCTW, GWLP_USERDATA, GetWindowLongPtrW, SetWindowLongPtrW,
};

/// A window's state, tagged with its type.
#[repr(C)]
pub(super) struct WindowData<T: 'static> {
    /// First, so it can be read without knowing `T`.
    kind: TypeId,
    value: T,
}

impl<T: 'static> WindowData<T> {
    pub(super) fn new(value: T) -> Self {
        Self {
            kind: TypeId::of::<T>(),
            value,
        }
    }
}

impl<T: 'static> Deref for WindowData<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: 'static> DerefMut for WindowData<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.value
    }
}

/// A state handed to a new window through `CreateWindowExW`'s
/// `lpCreateParams`. The window's `WM_NCCREATE` takes it
/// ([`adopt_create_param`]) and owns it from then on; if creation fails
/// before that, dropping this drops the state.
pub(super) struct CreateParam<T: 'static>(Option<Box<WindowData<T>>>);

impl<T: 'static> CreateParam<T> {
    pub(super) fn new(value: T) -> Self {
        Self(Some(Box::new(WindowData::new(value))))
    }

    /// The `lpCreateParams` value. `self` must outlive `CreateWindowExW`.
    pub(super) fn as_param(&mut self) -> *const c_void {
        (self as *mut Self).cast()
    }
}

/// In `WM_NCCREATE`: makes the `CreateParam<T>` passed to `CreateWindowExW`
/// the window's own state. [`drop_owned`] frees it in `WM_NCDESTROY`.
pub(super) unsafe fn adopt_create_param<T: 'static>(hwnd: HWND, lparam: LPARAM) {
    let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
    let param = unsafe { &mut *create.lpCreateParams.cast::<CreateParam<T>>() };
    let data = param.0.take().expect("a window adopts its state once");
    unsafe { attach(hwnd, Box::into_raw(data)) };
}

/// Makes `value` the window's own state. [`drop_owned`] frees it.
pub(super) unsafe fn adopt<T: 'static>(hwnd: HWND, value: T) {
    unsafe { attach(hwnd, Box::into_raw(Box::new(WindowData::new(value)))) };
}

/// Frees the state the window owns, if it was given one.
pub(super) unsafe fn drop_owned<T: 'static>(hwnd: HWND) {
    let data = unsafe { detach::<T>(hwnd) };
    if !data.is_null() {
        drop(unsafe { Box::from_raw(data) });
    }
}

/// In `WM_NCCREATE`: lends the window the `WindowData<T>` passed as
/// `lpCreateParams` ([`lend`]).
pub(super) unsafe fn borrow_create_param<T: 'static>(hwnd: HWND, lparam: LPARAM) {
    let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
    unsafe { lend(hwnd, create.lpCreateParams.cast::<WindowData<T>>()) };
}

/// Gives the window `data`, which the caller owns and keeps alive until the
/// window is destroyed or [`detach`]ed.
pub(super) unsafe fn lend<T: 'static>(hwnd: HWND, data: *mut WindowData<T>) {
    unsafe { attach(hwnd, data) };
}

unsafe fn attach<T: 'static>(hwnd: HWND, data: *mut WindowData<T>) {
    assert_eq!(
        unsafe { (*data).kind },
        TypeId::of::<T>(),
        "a window is given a {}",
        std::any::type_name::<T>()
    );
    assert_eq!(
        unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) },
        0,
        "window {hwnd:?} already has its state"
    );
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, data as isize) };
}

/// The window's state, or `None` before it is attached or after it is
/// detached. Panics if the window holds another type of state.
pub(super) unsafe fn get<T: 'static>(hwnd: HWND) -> Option<&'static mut T> {
    let data = unsafe { data_ptr::<T>(hwnd) };
    (!data.is_null()).then(|| unsafe { &mut (*data).value })
}

/// Clears the window's state and returns it (null if it had none).
pub(super) unsafe fn detach<T: 'static>(hwnd: HWND) -> *mut WindowData<T> {
    let data = unsafe { data_ptr::<T>(hwnd) };
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
    data
}

unsafe fn data_ptr<T: 'static>(hwnd: HWND) -> *mut WindowData<T> {
    let data = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowData<T>;
    if !data.is_null() {
        // `kind` is at the same offset in every `WindowData`.
        let kind = unsafe { (*data.cast::<WindowData<()>>()).kind };
        assert_eq!(
            kind,
            TypeId::of::<T>(),
            "window {hwnd:?} holds another kind of state than {}",
            std::any::type_name::<T>()
        );
    }
    data
}
