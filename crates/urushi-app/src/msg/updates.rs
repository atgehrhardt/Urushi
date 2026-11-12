//! Messages for updating the app (ADR 0049): the update notice, its confirmation, and
//! the Updates section of Settings.

/// Something about app updates happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMsg {
    /// "Later" on the notice: don't offer this release again (a newer one is).
    Later,
    /// "Update…": show what updating stops, and ask.
    Open,
    /// Confirmed: install the release, then restart the daemon and the app.
    Confirm,
    /// Settings: look for a new release now.
    CheckNow,
    /// Settings: switch automatic checks on or off.
    ToggleAutomatic,
}
