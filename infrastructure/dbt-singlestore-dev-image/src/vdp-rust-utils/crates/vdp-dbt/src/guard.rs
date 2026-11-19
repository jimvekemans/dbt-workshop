//! Refuses `dbt build` / `dbt run` of the whole project, unless a service user runs it.

use crate::argv::Scan;
use crate::settings::DbtSettings;

/// Why an invocation may run, or why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The command is not guarded (or there is none: dbt itself will explain).
    NotGuarded,
    /// A selector flag is present.
    Selected,
    /// The SingleStore user is a service user.
    ServiceUser,
    /// A guarded command without a selector, by a personal user.
    Refused { command: String },
}

impl Verdict {
    /// Whether the command may run (every verdict but [`Verdict::Refused`]).
    pub const fn allowed(&self) -> bool {
        !matches!(self, Self::Refused { .. })
    }
}

/// Decides from the scan alone; argv is never changed by the guard.
pub fn decide(scan: &Scan, settings: &DbtSettings, singlestore_user: Option<&str>) -> Verdict {
    let Some(command) = scan.command() else {
        return Verdict::NotGuarded;
    };
    // `dbt build --help` only prints help.
    if scan.uses("help") || scan.uses("version") {
        return Verdict::NotGuarded;
    }
    if !settings.guarded_commands.iter().any(|c| c == command) {
        return Verdict::NotGuarded;
    }
    if scan.uses_any(&settings.selector_flags) {
        return Verdict::Selected;
    }
    if singlestore_user.is_some_and(|user| settings.is_service_user(user)) {
        return Verdict::ServiceUser;
    }
    Verdict::Refused {
        command: command.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;
    use crate::argv::scan;
    use crate::flags::Grammar;

    fn verdict(words: &[&str], user: Option<&str>) -> Verdict {
        let argv: Vec<OsString> = words.iter().map(OsString::from).collect();
        decide(
            &scan(&argv, &Grammar::embedded()),
            &DbtSettings::default(),
            user,
        )
    }

    #[test]
    fn python_rules() {
        assert!(!verdict(&["build"], None).allowed());
        assert!(!verdict(&["run", "--exclude", "x"], Some("jdoe")).allowed());
        assert_eq!(verdict(&["build", "-s", "x"], None), Verdict::Selected);
        assert_eq!(verdict(&["run", "--select=x"], None), Verdict::Selected);
        assert_eq!(verdict(&["build"], Some("svc_dbt")), Verdict::ServiceUser);
        assert!(!verdict(&["build"], Some("pu_dbt")).allowed());
        assert_eq!(verdict(&["test"], None), Verdict::NotGuarded);
        assert_eq!(verdict(&[], None), Verdict::NotGuarded);
    }

    #[test]
    fn bypasses_the_python_guard_missed() {
        assert!(!verdict(&["--debug", "build"], None).allowed());
        assert!(!verdict(&["-d", "build"], None).allowed());
        assert!(!verdict(&["--target", "build", "build"], None).allowed());
        assert!(!verdict(&["--profiles-dir", ".", "run"], None).allowed());
        assert!(!verdict(&["build", "--", "--select", "x"], None).allowed());
        assert_eq!(
            verdict(&["build", "--selector", "daily"], None),
            Verdict::Selected
        );
        assert_eq!(
            verdict(&["--select", "x", "build"], None),
            Verdict::NotGuarded
        );
        assert_eq!(verdict(&["build", "--help"], None), Verdict::NotGuarded);
        assert_eq!(verdict(&["run", "-h"], None), Verdict::NotGuarded);
    }
}
