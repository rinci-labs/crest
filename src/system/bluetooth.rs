//! Notices when paired Bluetooth devices connect or disconnect, including the battery
//! percentage when the device reports it.

use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationKind, DeviceInformationUpdate, DeviceWatcher,
};
use windows::Foundation::{IPropertyValue, TypedEventHandler};
use windows::core::{GUID, HSTRING, IInspectable, Interface, Result};
use windows_collections::{IIterable, IMapView};

use crate::system::services::{Event, Notifier, Task};

const IS_CONNECTED: &str = "System.Devices.Aep.IsConnected";
const COD_MAJOR: &str = "System.Devices.Aep.Bluetooth.Cod.Major";
const CONTAINER: &str = "System.Devices.Aep.ContainerId";
/// Battery property used by the Bluetooth page in Settings.
const BATTERY: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
/// Classic Bluetooth or Bluetooth LE, paired devices only.
const PAIRED_DEVICES: &str = "(System.Devices.Aep.ProtocolId:=\"{e0cbf06c-cd8b-4647-bb8a-263b43f0f974}\" OR \
     System.Devices.Aep.ProtocolId:=\"{bb7bb05e-5972-42b5-94fc-76eaa7084d49}\") AND \
     System.Devices.Aep.IsPaired:=System.StructuredQueryType.Boolean#True";
/// Headset battery is usually readable only a few seconds after connecting.
const BATTERY_RETRY: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Audio,
    Input,
    Phone,
    Other,
}

pub struct Notice {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub kind: DeviceKind,
}

/// Changes from `DeviceWatcher`, converted to plain data.
pub enum Change {
    Added(DeviceData),
    Updated(DeviceData),
    Removed(String),
    /// The initial list has been read; from now on changes are worth announcing.
    Ready,
    BatteryRetry(String),
}

pub struct DeviceData {
    id: String,
    name: Option<String>,
    connected: Option<bool>,
    major: Option<u32>,
    container: Option<GUID>,
}

struct Device {
    name: String,
    connected: bool,
    kind: DeviceKind,
    container: Option<GUID>,
    /// A delayed battery read is already scheduled; reconnect storms must not pile up threads.
    retry_pending: bool,
}

pub struct Monitor {
    watcher: DeviceWatcher,
    devices: HashMap<String, Device>,
    ready: bool,
    tasks: Sender<Task>,
    notify: Notifier,
}

impl Monitor {
    pub fn new(tasks: Sender<Task>, notify: Notifier) -> Result<Self> {
        let properties = string_list(&[IS_CONNECTED, COD_MAJOR, CONTAINER]);
        let watcher = DeviceInformation::CreateWatcherWithKindAqsFilterAndAdditionalProperties(
            &HSTRING::from(PAIRED_DEVICES),
            &properties,
            DeviceInformationKind::AssociationEndpoint,
        )?;

        let t = tasks.clone();
        watcher.Added(&TypedEventHandler::new(move |_, info: windows::core::Ref<DeviceInformation>| {
            if let Some(info) = info.as_ref() {
                let _ = t.send(Task::Bluetooth(Change::Added(from_info(info))));
            }
            Ok(())
        }))?;
        let t = tasks.clone();
        watcher.Updated(&TypedEventHandler::new(move |_, update: windows::core::Ref<DeviceInformationUpdate>| {
            if let Some(update) = update.as_ref() {
                let _ = t.send(Task::Bluetooth(Change::Updated(from_update(update))));
            }
            Ok(())
        }))?;
        let t = tasks.clone();
        watcher.Removed(&TypedEventHandler::new(move |_, update: windows::core::Ref<DeviceInformationUpdate>| {
            if let Some(id) = update.as_ref().and_then(|u| u.Id().ok()) {
                let _ = t.send(Task::Bluetooth(Change::Removed(id.to_string())));
            }
            Ok(())
        }))?;
        let t = tasks.clone();
        watcher.EnumerationCompleted(&TypedEventHandler::new(move |_, _| {
            let _ = t.send(Task::Bluetooth(Change::Ready));
            Ok(())
        }))?;
        watcher.Start()?;
        Ok(Self { watcher, devices: HashMap::new(), ready: false, tasks, notify })
    }

    pub fn apply(&mut self, change: Change) {
        match change {
            Change::Added(data) => {
                let connected = data.connected.unwrap_or(false);
                let device = Device {
                    name: data.name.unwrap_or_default(),
                    connected,
                    kind: kind(data.major),
                    container: data.container,
                    retry_pending: false,
                };
                self.devices.insert(data.id.clone(), device);
                if self.ready && connected {
                    self.announce(&data.id);
                }
            }
            Change::Updated(data) => {
                let Some(device) = self.devices.get_mut(&data.id) else { return };
                if let Some(name) = data.name.filter(|n| !n.is_empty()) {
                    device.name = name;
                }
                if let Some(connected) = data.connected
                    && connected != device.connected
                {
                    device.connected = connected;
                    if self.ready {
                        self.announce(&data.id);
                    }
                }
            }
            Change::Removed(id) => {
                self.devices.remove(&id);
            }
            Change::Ready => self.ready = true,
            Change::BatteryRetry(id) => {
                let Some(device) = self.devices.get_mut(&id) else { return };
                device.retry_pending = false;
                if device.connected
                    && let Some(battery) = device.container.and_then(battery)
                {
                    self.notify.send(Event::BluetoothBattery { id, battery });
                }
            }
        }
    }

    fn announce(&mut self, id: &str) {
        let Some(device) = self.devices.get_mut(id) else { return };
        let battery = if device.connected { device.container.and_then(battery) } else { None };
        self.notify.send(Event::Bluetooth(Notice {
            id: id.to_string(),
            name: device.name.clone(),
            connected: device.connected,
            battery,
            kind: device.kind,
        }));
        if device.connected && battery.is_none() && device.container.is_some() && !device.retry_pending {
            device.retry_pending = true;
            let (tasks, id) = (self.tasks.clone(), id.to_string());
            thread::spawn(move || {
                thread::sleep(BATTERY_RETRY);
                let _ = tasks.send(Task::Bluetooth(Change::BatteryRetry(id)));
            });
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        let _ = self.watcher.Stop();
    }
}

fn from_info(info: &DeviceInformation) -> DeviceData {
    let mut data = read_properties(info.Properties().ok());
    data.id = info.Id().map(|s| s.to_string()).unwrap_or_default();
    data.name = info.Name().ok().map(|s| s.to_string());
    data
}

fn from_update(update: &DeviceInformationUpdate) -> DeviceData {
    let mut data = read_properties(update.Properties().ok());
    data.id = update.Id().map(|s| s.to_string()).unwrap_or_default();
    data
}

fn read_properties(map: Option<IMapView<HSTRING, IInspectable>>) -> DeviceData {
    let get = |key: &str| {
        map.as_ref()?.Lookup(&HSTRING::from(key)).ok()?.cast::<IPropertyValue>().ok()
    };
    DeviceData {
        id: String::new(),
        name: None,
        connected: get(IS_CONNECTED).and_then(|v| v.GetBoolean().ok()),
        major: get(COD_MAJOR).and_then(|v| v.GetUInt32().or_else(|_| v.GetUInt16().map(u32::from)).ok()),
        container: get(CONTAINER).and_then(|v| v.GetGuid().ok()),
    }
}

/// Battery is read from the device node (not the endpoint) in the same container.
fn battery(container: GUID) -> Option<u8> {
    let filter = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{container:?}}}\""));
    let devices = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(
        &filter,
        &string_list(&[BATTERY]),
        DeviceInformationKind::Device,
    )
    .ok()?
    .join()
    .ok()?;
    let key = HSTRING::from(BATTERY);
    (0..devices.Size().ok()?).find_map(|i| {
        let value = devices.GetAt(i).ok()?.Properties().ok()?.Lookup(&key).ok()?;
        value.cast::<IPropertyValue>().ok()?.GetUInt8().ok().filter(|b| *b <= 100)
    })
}

/// Bluetooth major class (Class of Device), used to pick the icon.
fn kind(major: Option<u32>) -> DeviceKind {
    match major {
        Some(4) => DeviceKind::Audio,
        Some(5) => DeviceKind::Input,
        Some(2) => DeviceKind::Phone,
        _ => DeviceKind::Other,
    }
}

fn string_list(items: &[&str]) -> IIterable<HSTRING> {
    IIterable::from(items.iter().map(|s| HSTRING::from(*s)).collect::<Vec<_>>())
}
