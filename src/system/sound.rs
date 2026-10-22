//! Alert sounds using the built-in Windows sounds. They follow the sound scheme chosen in
//! Windows settings, so no sound files of our own are needed, and stay silent while Windows is
//! in do-not-disturb (presenting, full-screen game or video, quiet hours).

use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_QUIET_TIME, QUNS_RUNNING_D3D_FULL_SCREEN,
    SHQueryUserNotificationState,
};
use windows::core::{PCWSTR, w};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sound {
    /// Water reminder.
    Reminder,
    /// Focus phase finished.
    FocusDone,
    /// Bluetooth device connected, charger plugged in.
    Connect,
    /// Bluetooth device disconnected, charger unplugged.
    Disconnect,
    LowBattery,
    /// An app started using the camera/microphone.
    Privacy,
    /// Download finished.
    Download,
    /// Quick timer ran out.
    Alarm,
    /// Any other notice that needs no sound of its own (screenshot saved, Wi-Fi lost internet).
    Notice,
}

impl Sound {
    /// Windows sound event name (Control Panel → Sound).
    fn alias(self) -> PCWSTR {
        match self {
            Sound::Reminder => w!("Notification.Reminder"),
            Sound::FocusDone => w!("Notification.Default"),
            Sound::Connect => w!("DeviceConnect"),
            Sound::Disconnect => w!("DeviceDisconnect"),
            Sound::LowBattery => w!("LowBatteryAlarm"),
            Sound::Privacy => w!("SystemNotification"),
            Sound::Alarm => w!("Notification.Looping.Alarm"),
            Sound::Download => w!("Notification.Default"),
            Sound::Notice => w!("Notification.Default"),
        }
    }
}

pub fn play(sound: Sound) {
    if quiet() {
        return;
    }
    unsafe {
        let _ = PlaySoundW(sound.alias(), None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT);
    }
}

/// Stop the sound Crest is playing (the timer alarm is several seconds long).
pub fn stop() {
    unsafe {
        let _ = PlaySoundW(PCWSTR::null(), None, SND_ASYNC);
    }
}

fn quiet() -> bool {
    matches!(
        unsafe { SHQueryUserNotificationState() },
        Ok(QUNS_BUSY | QUNS_RUNNING_D3D_FULL_SCREEN | QUNS_PRESENTATION_MODE | QUNS_QUIET_TIME)
    )
}
