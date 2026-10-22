//! Speaker volume and microphone mute: listen for changes on the default devices, follow the
//! default device automatically when it changes (e.g. a Bluetooth headset connects), and change
//! both from the quick controls page.

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eCapture, eConsole, eMultimedia,
    eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::{PCWSTR, Result, implement};

use crate::system::services::{Event, Notifier, Task};

type Endpoint = (IAudioEndpointVolume, IAudioEndpointVolumeCallback);

pub struct Monitor {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    endpoint: Option<Endpoint>,
    /// Default microphone, for the mute button.
    mic: Option<Endpoint>,
    notify: Notifier,
}

impl Monitor {
    pub fn new(tasks: Sender<Task>, notify: Notifier) -> Result<Self> {
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let client: IMMNotificationClient = DeviceClient { tasks }.into();
            enumerator.RegisterEndpointNotificationCallback(&client)?;
            let mut monitor = Self { enumerator, client, endpoint: None, mic: None, notify };
            monitor.attach_default();
            Ok(monitor)
        }
    }

    /// Listen to the current default speaker volume and microphone mute.
    pub fn attach_default(&mut self) {
        self.detach();
        match attach(&self.enumerator, &self.notify) {
            Ok(endpoint) => self.endpoint = Some(endpoint),
            Err(e) => error_log!("cannot monitor the default audio device: {e}"),
        }
        self.mic = attach_mic(&self.enumerator, &self.notify).ok();
        if self.mic.is_none() {
            self.notify.send(Event::Mic(None));
        }
    }

    pub fn set_volume(&self, level: f32) {
        if let Some((volume, _)) = &self.endpoint {
            unsafe {
                let _ = volume.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null());
                // Moving the volume also unmutes, like the Windows slider.
                let _ = volume.SetMute(false, std::ptr::null());
            }
        }
    }

    pub fn toggle_mute(&self) {
        if let Some((volume, _)) = &self.endpoint {
            unsafe {
                if let Ok(muted) = volume.GetMute() {
                    let _ = volume.SetMute(!muted.as_bool(), std::ptr::null());
                }
            }
        }
    }

    pub fn toggle_mic(&self) {
        if let Some((mic, _)) = &self.mic {
            unsafe {
                if let Ok(muted) = mic.GetMute() {
                    let _ = mic.SetMute(!muted.as_bool(), std::ptr::null());
                }
            }
        }
    }

    fn detach(&mut self) {
        for (volume, callback) in [self.endpoint.take(), self.mic.take()].into_iter().flatten() {
            let _ = unsafe { volume.UnregisterControlChangeNotify(&callback) };
        }
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.detach();
        let _ = unsafe { self.enumerator.UnregisterEndpointNotificationCallback(&self.client) };
    }
}

fn attach(enumerator: &IMMDeviceEnumerator, notify: &Notifier) -> Result<Endpoint> {
    unsafe {
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
        // The initial value is kept so an identical first notification does not pop the HUD,
        // and is sent as the initial state for the slider on the controls page.
        let (level, muted) = (volume.GetMasterVolumeLevelScalar()?, volume.GetMute()?.as_bool());
        notify.send(Event::Volume { level, muted, initial: true });
        let callback: IAudioEndpointVolumeCallback =
            VolumeCallback { notify: notify.clone(), last: Mutex::new(Some(key(level, muted))) }.into();
        volume.RegisterControlChangeNotify(&callback)?;
        Ok((volume, callback))
    }
}

fn attach_mic(enumerator: &IMMDeviceEnumerator, notify: &Notifier) -> Result<Endpoint> {
    unsafe {
        let device = enumerator.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        let volume: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None)?;
        let muted = volume.GetMute()?.as_bool();
        notify.send(Event::Mic(Some(muted)));
        let callback: IAudioEndpointVolumeCallback = MicCallback { notify: notify.clone(), last: Mutex::new(muted) }.into();
        volume.RegisterControlChangeNotify(&callback)?;
        Ok((volume, callback))
    }
}

/// Volume rounded to percent so near-identical notifications do not count as changes.
fn key(level: f32, muted: bool) -> (u32, bool) {
    ((level * 100.0).round() as u32, muted)
}

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeCallback {
    notify: Notifier,
    last: Mutex<Option<(u32, bool)>>,
}

impl IAudioEndpointVolumeCallback_Impl for VolumeCallback_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        let Some(data) = (unsafe { data.as_ref() }) else { return Ok(()) };
        let (level, muted) = (data.fMasterVolume, data.bMuted.as_bool());
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if *last != Some(key(level, muted)) {
            *last = Some(key(level, muted));
            self.notify.send(Event::Volume { level, muted, initial: false });
        }
        Ok(())
    }
}

/// Only the microphone mute state is reported; its recording level is unused.
#[implement(IAudioEndpointVolumeCallback)]
struct MicCallback {
    notify: Notifier,
    last: Mutex<bool>,
}

impl IAudioEndpointVolumeCallback_Impl for MicCallback_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> Result<()> {
        let Some(data) = (unsafe { data.as_ref() }) else { return Ok(()) };
        let muted = data.bMuted.as_bool();
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if *last != muted {
            *last = muted;
            self.notify.send(Event::Mic(Some(muted)));
        }
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceClient {
    tasks: Sender<Task>,
}

impl IMMNotificationClient_Impl for DeviceClient_Impl {
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
        // A device was plugged/unplugged; the microphone monitor also needs to refresh.
        let _ = self.tasks.send(Task::AudioDeviceChanged);
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> {
        Ok(())
    }

    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> {
        Ok(())
    }

    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> Result<()> {
        if (flow == eRender && role == eMultimedia) || (flow == eCapture && role == eConsole) {
            let _ = self.tasks.send(Task::AudioDeviceChanged);
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> {
        Ok(())
    }
}
