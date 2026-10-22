//! Wi-Fi state for connected/disconnected alerts. Windows reports every network change; the
//! state is read only once the changes settle (see `services`). The network name comes from its
//! profile name, so no location permission is needed.

use std::sync::mpsc::Sender;

use windows::Networking::Connectivity::{
    ConnectionProfile, NetworkConnectivityLevel, NetworkInformation, NetworkStatusChangedEventHandler,
};
use windows::core::Result;

use crate::system::services::Task;

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct State {
    /// Name of the connected Wi-Fi network; `None` when not on Wi-Fi.
    pub wifi: Option<String>,
    pub internet: bool,
}

pub struct Monitor {
    token: i64,
}

impl Monitor {
    pub fn new(tasks: Sender<Task>) -> Result<Self> {
        let handler = NetworkStatusChangedEventHandler::new(move |_| {
            let _ = tasks.send(Task::NetworkChanged);
            Ok(())
        });
        Ok(Self { token: NetworkInformation::NetworkStatusChanged(&handler)? })
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = NetworkInformation::RemoveNetworkStatusChanged(self.token);
    }
}

pub fn state() -> State {
    // The profile used for internet; failing that, a Wi-Fi connected without internet.
    let profile = NetworkInformation::GetInternetConnectionProfile().ok().or_else(connected_wifi);
    let Some(profile) = profile else { return State::default() };
    let level = profile.GetNetworkConnectivityLevel().unwrap_or(NetworkConnectivityLevel::None);
    let wifi = profile
        .IsWlanConnectionProfile()
        .unwrap_or(false)
        .then(|| profile.ProfileName().map(|name| name.to_string()).unwrap_or_default());
    State { wifi, internet: level == NetworkConnectivityLevel::InternetAccess }
}

fn connected_wifi() -> Option<ConnectionProfile> {
    let profiles = NetworkInformation::GetConnectionProfiles().ok()?;
    profiles.into_iter().find(|p| {
        p.IsWlanConnectionProfile().unwrap_or(false)
            && p.GetNetworkConnectivityLevel().is_ok_and(|level| level != NetworkConnectivityLevel::None)
    })
}
