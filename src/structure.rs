use std::time::Duration;

use zbus::zvariant::OwnedObjectPath;

pub type BSSID = String;

#[derive(PartialEq, Debug)]

// REMOVE: This signal is exclusively for AppState::Scanning;

// UPDATE: This signal is for any background task, that overrides UX

// This doesn't stop the scan, but rather stops the visual feedback gracefully.
// It ensures that it won't override other visual feedbacks.
// It is much more reliable than SourceIdW::remove(self).

// If any function wants to shut down a background task, it will set the ShutGraceful signal.
// When the background task receives this signal,
// it must respond with RespondToShut.
// After receiving RespondToShut.

// Before running a background task, set the CanRun signal. This must be done exactly before execution.
// This is to determine if there is any task that has already been shut down, so the shut() function
// won't get stuck in an infinite loop when other functions close the task.
// A layer can be created (e.g., execute_task()) that handles this automatically,
// but if there aren't many tasks, and it is compratively ez to do.

// After setting CanRun, a background task can run at any time. It simply indicates that upcoming background tasks can run.

// This may cause unexpected functions to close or run unpredictably, which can be difficult to debug. So, be cautious.
pub enum FnSIG {
    // Denotes that a function can run when called, and must be excatly one line before executing
    CanRun,
    // Denotes that a function needs to shut down gracefully.
    ShutGracefull,
    // The function that is going to be shut down responds with this signal to indicate that scanning has been shut down.
    RespondToShut,
}

// Used to handle by same one signal, now it has been splited.
#[derive(PartialEq, Debug)]
pub struct ExecutionSignals {
    pub scan_task: FnSIG,
    pub connect_task: FnSIG,
}

// VFB => Visual FeedBack
pub enum VFBTask {
    Scan,
    Connect,
}

impl VFBTask {
    pub fn poll_signal(&self, signals: &mut ExecutionSignals) -> bool {
        let task_signal = self.task_signal(signals);
        if *task_signal == FnSIG::ShutGracefull || *task_signal == FnSIG::RespondToShut {
            *task_signal = FnSIG::RespondToShut;
            return true;
        }
        return false;
    }

    fn task_signal<'a>(&self, signals: &'a mut ExecutionSignals) -> &'a mut FnSIG {
        match self {
            VFBTask::Scan => &mut signals.scan_task,
            VFBTask::Connect => &mut signals.connect_task,
        }
    }
}
// name speaks itself
#[derive(PartialEq, Debug)]
pub enum AppState {
    /// The application is not doing anything
    Idle,
    /// The application is scanning for available Wi-Fi networks.
    Scanning,
    /// The application is trying to connect to a specific Wi-Fi network, represented by the SSID.
    Connecting(BSSID),
    /// The application is waiting for the user to input the Wi-Fi password. The retry field determines if this is a retry attempt after a failed connection.
    PasswordInput {
        bssid: String,
        // previous connection stateReason
        // if reason is NO_SECRETS i.e 7, then bad auth: should be shown as a prompt
        // else
        reason: u32,
    },
}

#[derive(Debug, Clone)]
// List of available aps
pub struct AccessPoint {
    /// The name of the Wi-Fi network
    pub ssid: String,
    /// This MAC address of the WiFi network.
    pub bssid: BSSID,
    /// The frequency of the Wi-Fi network in MHz
    #[allow(unused)]
    pub frequency: u32,
    /// The signal strength of the Wi-Fi network.
    pub signal_strength: u8,
    // Whether the network requires a password to connect.
    pub is_protected: bool,
    // Whether the network configuration exits.
    pub setting_path: Option<zbus::zvariant::OwnedObjectPath>,
}

#[derive(Debug)]
// Data for rendering a loading animation during Wi-Fi scanning.
pub struct IndicatorAnim {
    /// A list of strings that represent different frames of the animation (e.g., [".", "..", "..."]).
    ///
    ///
    /// CHANGED Pre builds a frame in initilization and store in this vector.
    /// REMOVED: used to store indicator directly, and used to lazely build it.
    pub frames: Vec<std::ffi::CString>,
    /// The current index in the frames array, used to cycle through the animation.
    pub index: usize,
    ///
    pub fps: u8,
}

#[derive(Debug)]
// This stores blocking and must be converted into an async function.
// Converting to async is cheap since both are just thin wrappers around the same connection.
pub struct NetworkManagerDbusProxy {
    pub con: zbus::Connection, //clone of con is cheap
    pub wifi_proxy: zbus::Proxy<'static>,
    pub property_proxy: zbus::fdo::PropertiesProxy<'static>,
    pub dev_path: zbus::zvariant::OwnedObjectPath,
}

#[derive(Debug)]
// Represents Wi-Fi icons for different security modes
pub struct WiFiIcon {
    pub open: Vec<char>,
    pub psk: Vec<char>,
}

impl Default for AppState {
    fn default() -> Self {
        AppState::Idle
    }
}

impl IndicatorAnim {
    pub fn build_scan(display_name: &str, frame: &str) -> std::ffi::CString {
        std::ffi::CString::new(format!("{} {}", frame, display_name)).unwrap()
    }
    // pub fn build_connect(display_name: &str, frame: &str) -> std::ffi::CString {
    //     std::ffi::CString::new(format!("{}{}", display_name, frame)).unwrap()
    // }
    pub fn build_connect(frame: &str) -> std::ffi::CString {
        //  Remove the display_name, a name must be attached within frame
        std::ffi::CString::new(frame).unwrap()
    }
}

impl Default for WiFiIcon {
    fn default() -> Self {
        WiFiIcon {
            open: vec!['󰤨', '󰤥', '󰤢', '󰤟', '󰤯'],
            psk: vec!['󰤪', '󰤧', '󰤤', '󰤡', '󰤬'],
        }
    }
}
// Basically, The entire state of the application

#[derive(Debug)]
pub struct PrivateData {
    pub anim_scan: IndicatorAnim,
    pub anim_connecting: IndicatorAnim,
    pub aps: Vec<AccessPoint>,
    // leaked from rust gc
    #[allow(unused)]
    leaked_display_values: Vec<*mut std::ffi::CString>,
    pub state: AppState,
    pub icons: WiFiIcon,
    pub active_connection: Option<BSSID>,
    pub nm_dbus: NetworkManagerDbusProxy,
    pub display_name: std::ffi::CString,
    pub hidden_ssid: Option<String>,
    _execution_signal: ExecutionSignals,
}

impl PrivateData {
    pub fn new(
        network_manager_proxy: NetworkManagerDbusProxy,
        cached_aps: Vec<AccessPoint>,
    ) -> Self {
        Self {
            anim_scan: IndicatorAnim {
                frames: vec![
                    IndicatorAnim::build_scan("wifi", "⠻"),
                    IndicatorAnim::build_scan("wifi", "⠽"),
                    IndicatorAnim::build_scan("wifi", "⠾"),
                    IndicatorAnim::build_scan("wifi", "⠷"),
                    IndicatorAnim::build_scan("wifi", "⠯"),
                    IndicatorAnim::build_scan("wifi", "⠟"),
                ],
                index: 0,
                fps: 10,
            },
            anim_connecting: IndicatorAnim {
                frames: vec![
                    IndicatorAnim::build_connect("connecting ."),
                    IndicatorAnim::build_connect("connecting .."),
                    IndicatorAnim::build_connect("connecting ..."),
                ],
                index: 0,
                fps: 4,
            },
            aps: cached_aps,
            nm_dbus: network_manager_proxy,
            active_connection: None,
            hidden_ssid: None,
            _execution_signal: ExecutionSignals {
                scan_task: FnSIG::CanRun,
                connect_task: FnSIG::CanRun,
            },
            display_name: std::ffi::CString::new("wifi").unwrap(),
            icons: WiFiIcon::default(),
            leaked_display_values: Vec::new(),
            state: AppState::Idle,
        }
    }

    async fn shut(field: &mut FnSIG, fps: u8) {
        if *field == FnSIG::RespondToShut {
            // function is already shutdown.
            return;
        }

        *field = FnSIG::ShutGracefull;
        loop {
            glib::timeout_future(Duration::from_millis(1000 / fps as u64)).await;

            {
                if *field == FnSIG::RespondToShut {
                    break;
                }
            }
        }
    }

    pub fn shut_scan(&mut self) -> impl Future<Output = ()> {
        Self::shut(&mut self._execution_signal.scan_task, self.anim_scan.fps)
    }

    pub fn shut_connect(&mut self) -> impl Future<Output = ()> {
        Self::shut(
            &mut self._execution_signal.connect_task,
            self.anim_connecting.fps,
        )
    }

    pub fn allow_execute(&mut self, task: VFBTask) {
        match task {
            VFBTask::Connect => self._execution_signal.connect_task = FnSIG::CanRun,
            VFBTask::Scan => self._execution_signal.scan_task = FnSIG::CanRun,
        }
    }

    pub fn pool_shut_signal(&mut self, task: VFBTask) -> bool {
        match task {
            VFBTask::Connect => VFBTask::Connect.poll_signal(&mut self._execution_signal),
            VFBTask::Scan => VFBTask::Scan.poll_signal(&mut self._execution_signal),
        }
    }

    pub fn sort_accesspoints(&mut self) {
        self.aps
            .sort_by(|a, b| (b.signal_strength).cmp(&a.signal_strength));

        let connected_index = self.aps.iter().position(|x| match &self.state {
            AppState::Connecting(bssid) => *x.bssid == *bssid,
            _ => match &self.active_connection {
                None => false,
                Some(v) => *v == *x.bssid,
            },
        });

        if let Some(index) = connected_index {
            if index != 0 {
                self.aps.swap(index, 0);
            }
        }
    }
    pub fn set_connected(&mut self, signature: Option<(BSSID, OwnedObjectPath)>) {
        if let Some((bssid, config)) = signature {
            if let Some(ap) = self.aps.iter_mut().find(|ap| ap.bssid == bssid) {
                ap.setting_path = Some(config);
            }
            self.active_connection = Some(bssid);
        }
    }
}
