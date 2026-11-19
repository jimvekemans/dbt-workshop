/// Emits exactly one milestone for the scope it guards: `completed` through [`complete`](Self::complete),
/// or `failed` (WARN) when it is dropped still armed, which covers every early return and `?`.
///
/// It only observes: it never changes what the guarded code does.
#[derive(Debug)]
pub(super) struct MilestoneGuard {
    subject: String,
    fail_detail: String,
    armed: bool,
}

impl MilestoneGuard {
    pub(super) fn new(subject: impl Into<String>, fail_detail: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            fail_detail: fail_detail.into(),
            armed: true,
        }
    }

    pub(super) fn complete(&mut self, detail: impl std::fmt::Display) {
        self.armed = false;
        vdp_core::milestone::completed(&self.subject, detail);
    }

    pub(super) const fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for MilestoneGuard {
    fn drop(&mut self) {
        if self.armed {
            vdp_core::milestone::failed(&self.subject, &self.fail_detail);
        }
    }
}
