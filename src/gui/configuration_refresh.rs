//! A saved configuration can invalidate an already running read. Keep one
//! follow-up read, with explicit draft rebasing taking precedence.
#[derive(Default)]
pub(super) struct PendingRefresh(Option<bool>);
impl PendingRefresh {
    pub fn request(&mut self, rebase: bool) {
        self.0 = Some(self.0.unwrap_or(false) || rebase);
    }
    pub fn take(&mut self) -> Option<bool> {
        self.0.take()
    }
}

pub(super) struct PreservedEditor {
    pub entry_id: String,
    pub values: serde_json::Value,
    pub base: (serde_json::Value, String),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_invalidations_merge_and_explicit_rebase_is_not_lost() {
        let mut pending = PendingRefresh::default();
        assert_eq!(pending.take(), None);
        pending.request(false);
        pending.request(false);
        assert_eq!(pending.take(), Some(false));
        pending.request(true);
        pending.request(false);
        assert_eq!(pending.take(), Some(true));
        assert_eq!(pending.take(), None);
    }
}
