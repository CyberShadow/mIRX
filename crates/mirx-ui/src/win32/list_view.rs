//! Report-style ListView helpers the dialogs share.

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Controls::{
    LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_STATE, LVIF_TEXT, LVIS_FOCUSED, LVIS_SELECTED, LVITEMW,
    LVM_DELETEALLITEMS, LVM_ENSUREVISIBLE, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMSTATE, LVM_SETITEMTEXTW, LVNI_SELECTED,
    LVS_EX_FULLROWSELECT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

use super::wide_null;

pub(super) const LIST_VIEW_CLASS_NAME: &str = "SysListView32";

/// Gives a new report-style list its columns, `(title, width)` each, and
/// whole-row selection.
pub(super) unsafe fn set_up_report_list(list: HWND, columns: &[(&str, i32)]) -> Result<(), String> {
    SendMessageW(
        list,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        LVS_EX_FULLROWSELECT as WPARAM,
        LVS_EX_FULLROWSELECT as LPARAM,
    );
    for (index, (title, width)) in columns.iter().enumerate() {
        let mut title = wide_null(title);
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            cx: *width,
            pszText: title.as_mut_ptr(),
            ..LVCOLUMNW::default()
        };
        if SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            index,
            &column as *const LVCOLUMNW as LPARAM,
        ) != index as LRESULT
        {
            return Err(format!("LVM_INSERTCOLUMNW {index} failed"));
        }
    }
    Ok(())
}

/// Fills a list's rows; nothing is selected afterwards, so the buttons
/// that act on a selected row are greyed until something is.
pub(super) unsafe fn fill_list(list: HWND, rows: &[Vec<String>]) {
    SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
    for (row, cells) in rows.iter().enumerate() {
        for (column, cell) in cells.iter().enumerate() {
            let mut text = wide_null(cell);
            let item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row as i32,
                iSubItem: column as i32,
                pszText: text.as_mut_ptr(),
                ..LVITEMW::default()
            };
            let message = if column == 0 {
                LVM_INSERTITEMW
            } else {
                LVM_SETITEMTEXTW
            };
            SendMessageW(list, message, row, &item as *const LVITEMW as LPARAM);
        }
    }
}

pub(super) unsafe fn selected_row(list: HWND) -> Option<usize> {
    let row = SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as LPARAM);
    usize::try_from(row).ok()
}

/// Selects (and focuses) `row`, as a click on it does.
pub(super) unsafe fn select_row(list: HWND, row: usize) {
    let state = LVIS_SELECTED | LVIS_FOCUSED;
    let item = LVITEMW {
        mask: LVIF_STATE,
        state,
        stateMask: state,
        ..LVITEMW::default()
    };
    SendMessageW(
        list,
        LVM_SETITEMSTATE,
        row,
        &item as *const LVITEMW as LPARAM,
    );
    SendMessageW(list, LVM_ENSUREVISIBLE, row, 0);
}

/// The automation harness's part of this module: what it reads and does
/// here for the protocol.
#[cfg(any(test, feature = "automation"))]
pub(super) mod automation {
    use super::*;
    use crate::automation::UiPoint;
    use crate::win32::room_list_window::automation::{rect_center, wide_buffer_text};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::Controls::{
        LVIR_LABEL, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMTEXTW,
    };

    /// A list's rows, `columns` cells each, and where a click selects each, in
    /// the list's client coordinates.
    pub(in crate::win32) unsafe fn dump_list(
        list: HWND,
        columns: usize,
    ) -> (Vec<Vec<String>>, Vec<UiPoint>) {
        let mut buffer = [0u16; 512];
        let rows = usize::try_from(SendMessageW(list, LVM_GETITEMCOUNT, 0, 0))
            .expect("LVM_GETITEMCOUNT succeeds");
        (0..rows)
            .map(|row| {
                let cells = (0..columns)
                    .map(|column| {
                        let mut item = LVITEMW {
                            iSubItem: column as i32,
                            pszText: buffer.as_mut_ptr(),
                            cchTextMax: buffer.len() as i32,
                            ..LVITEMW::default()
                        };
                        SendMessageW(list, LVM_GETITEMTEXTW, row, &mut item as *mut _ as LPARAM);
                        wide_buffer_text(&buffer)
                    })
                    .collect();
                let mut rect = RECT {
                    left: LVIR_LABEL as i32,
                    ..RECT::default()
                };
                assert_ne!(
                    SendMessageW(list, LVM_GETITEMRECT, row, &mut rect as *mut _ as LPARAM),
                    0,
                    "LVM_GETITEMRECT must place row {row}"
                );
                (cells, rect_center(&rect))
            })
            .unzip()
    }
}
