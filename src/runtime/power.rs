use std::{fs, path::Path};

/// Unknown deliberately has no effect on playback (e.g. on desktops or containers).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PowerState {
    #[default]
    Unknown,
    Ac,
    Battery,
}

impl PowerState {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Ac => "ac",
            Self::Battery => "battery",
        }
    }
}

/// Read the kernel's power-supply state without depending on a desktop power daemon.
/// An unreadable or incomplete sysfs never causes an aggressive power policy.
pub(crate) fn detect_power_state(root: &Path) -> PowerState {
    let Ok(entries) = fs::read_dir(root) else {
        return PowerState::Unknown;
    };
    let mut charger_seen = false;
    let mut charger_online = false;
    let mut battery_present = false;
    let mut battery_discharging = false;
    let mut battery_charging = false;

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = fs::read_to_string(path.join("type")) else {
            continue;
        };
        match kind.trim() {
            "Battery" => {
                if fs::read_to_string(path.join("present"))
                    .is_ok_and(|present| present.trim() == "0")
                {
                    continue;
                }
                battery_present = true;
                if let Ok(status) = fs::read_to_string(path.join("status")) {
                    battery_discharging |= status.trim() == "Discharging";
                    battery_charging |= status.trim() == "Charging";
                }
            }
            "Mains" | "USB" | "USB_C" | "USB_PD" | "USB_DCP" | "USB_CDP" | "USB_ACA" => {
                if let Ok(online) = fs::read_to_string(path.join("online")) {
                    charger_seen = true;
                    charger_online |= online.trim() == "1";
                }
            }
            _ => {}
        }
    }

    if charger_online || battery_charging {
        PowerState::Ac
    } else if battery_present && (battery_discharging || charger_seen) {
        PowerState::Battery
    } else {
        PowerState::Unknown
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{detect_power_state, PowerState};

    #[test]
    fn plugged_and_unplugged_states() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root =
            std::env::temp_dir().join(format!("we-layerd-power-{}-{unique}", std::process::id()));
        let battery = root.join("BAT0");
        let charger = root.join("AC0");
        fs::create_dir_all(&battery).unwrap();
        fs::create_dir_all(&charger).unwrap();
        fs::write(battery.join("type"), "Battery\n").unwrap();
        fs::write(battery.join("status"), "Discharging\n").unwrap();
        fs::write(charger.join("type"), "Mains\n").unwrap();
        fs::write(charger.join("online"), "0\n").unwrap();
        assert_eq!(detect_power_state(&root), PowerState::Battery);
        fs::write(charger.join("online"), "1\n").unwrap();
        assert_eq!(detect_power_state(&root), PowerState::Ac);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(detect_power_state(&root), PowerState::Unknown);
    }
}
