//! Native command input, with painting supplied by the application.
use crate::{
    event::{ButtonEvents, WindowEvents, WxEvtHandler},
    window::{WindowHandle, WxWidget},
};
use std::ffi::CString;
use wxdragon_sys as ffi;

#[derive(Clone, Copy)]
pub struct ActionButton {
    handle: WindowHandle,
}
impl ActionButton {
    pub fn set_default(&self) {
        if self.is_valid() {
            unsafe { ffi::wxd_ActionButton_SetDefault(self.handle_ptr()) };
        }
    }
    pub fn new(parent: &dyn WxWidget, id: i32, label: &str) -> Self {
        let label = CString::new(label).expect("button label contains NUL");
        let ptr = unsafe { ffi::wxd_ActionButton_Create(parent.handle_ptr(), id, label.as_ptr()) };
        assert!(!ptr.is_null());
        Self {
            handle: WindowHandle::new(ptr),
        }
    }
    pub fn set_presentation(&self, value: u32) {
        unsafe { ffi::wxd_ActionButton_SetPresentation(self.handle_ptr(), value) };
    }
    pub fn presentation(&self) -> u32 {
        unsafe { ffi::wxd_ActionButton_GetPresentation(self.handle_ptr()) }
    }
    pub fn visual_state(&self) -> u32 {
        unsafe { ffi::wxd_ActionButton_GetState(self.handle_ptr()) }
    }
}
impl WxWidget for ActionButton {
    fn handle_ptr(&self) -> *mut ffi::wxd_Window_t {
        self.handle.get_ptr().unwrap_or(std::ptr::null_mut())
    }
    fn is_valid(&self) -> bool {
        self.handle.is_valid()
    }
}
impl WxEvtHandler for ActionButton {
    unsafe fn get_event_handler_ptr(&self) -> *mut ffi::wxd_EvtHandler_t {
        self.handle_ptr().cast()
    }
}
impl WindowEvents for ActionButton {}
impl ButtonEvents for ActionButton {}
