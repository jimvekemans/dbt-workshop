/// How a command ended, mapped to the process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Everything succeeded (exit 0).
    Success,
    /// The command ran but some of its work failed (exit 1).
    Failures,
    /// Pass another process's exit code through unchanged (the dbt wrapper).
    Exit(u8),
}

impl Outcome {
    /// The process exit code: 0 for success, 1 for failures, the carried code for [`Outcome::Exit`].
    pub const fn exit_code(self) -> u8 {
        match self {
            Self::Success => 0,
            Self::Failures => 1,
            Self::Exit(code) => code,
        }
    }

    /// `Failures` when `ok` is false.
    pub const fn from_success(ok: bool) -> Self {
        if ok { Self::Success } else { Self::Failures }
    }
}
