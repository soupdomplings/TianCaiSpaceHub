//! Anchored native popup. Outside clicks and Escape dismiss it without a modal loop.
use crate::event::{WindowEvents, WxEvtHandler};
use crate::window::{WindowHandle, WxWidget};
use wxdragon_sys as ffi;

#[derive(Clone, Copy)]
pub struct TransientPopup {
    handle: WindowHandle,
}
impl TransientPopup {
    pub fn new(parent: &dyn WxWidget) -> Self {
        assert!(parent.is_valid());
        let ptr = unsafe { ffi::wxd_TransientPopup_Create(parent.handle_ptr()) };
        assert!(!ptr.is_null(), "Failed to create transient popup");
        Self {
            handle: WindowHandle::new(ptr),
        }
    }
    pub fn popup_for(&self, anchor: &dyn WxWidget, focus: &dyn WxWidget) {
        if self.is_valid() && anchor.is_valid() && focus.is_valid() {
            unsafe {
                ffi::wxd_TransientPopup_Show(
                    self.handle_ptr(),
                    anchor.handle_ptr(),
                    focus.handle_ptr(),
                )
            };
        }
    }
    pub fn dismiss(&self) {
        if self.is_valid() {
            unsafe { ffi::wxd_TransientPopup_Dismiss(self.handle_ptr()) };
        }
    }
}
impl WxWidget for TransientPopup {
    fn handle_ptr(&self) -> *mut ffi::wxd_Window_t {
        self.handle.get_ptr().unwrap_or(std::ptr::null_mut())
    }
    fn is_valid(&self) -> bool {
        self.handle.is_valid()
    }
}
impl WxEvtHandler for TransientPopup {
    unsafe fn get_event_handler_ptr(&self) -> *mut ffi::wxd_EvtHandler_t {
        self.handle_ptr().cast()
    }
}
impl WindowEvents for TransientPopup {}
