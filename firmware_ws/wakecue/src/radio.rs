//! The radio, started at most once per boot.
//!
//! Two jobs need it and neither runs without the other's leftovers being harmless: joining the
//! owner's network to set the clock, and raising the lamp's own network for the phone setup. The
//! scheduler and the heap the radio needs can only be started once, so the peripherals travel in
//! a [`Lazy`], and whichever job comes first starts it and the other reuses it.

use esp_hal::peripherals::{FROM_CPU_INTR0, TIMG0, WIFI};
use esp_hal::timer::timg::TimerGroup;
use esp_radio::wifi::WifiController;

/// The three peripherals the radio is built from.
pub struct Parts {
    pub wifi: WIFI<'static>,
    pub timer: TIMG0<'static>,
    pub interrupt: FROM_CPU_INTR0<'static>,
}

/// A started radio: the heap and the scheduler are up, and the controller has no configuration yet.
pub struct Radio {
    pub controller: WifiController<'static>,
}

/// The radio's parts, until somebody needs it.
pub struct Lazy {
    parts: Option<Parts>,
    radio: Option<Radio>,
}

impl Lazy {
    pub fn new(parts: Parts) -> Lazy {
        Lazy { parts: Some(parts), radio: None }
    }

    fn start(&mut self) {
        if self.radio.is_some() {
            return;
        }
        let parts = self.parts.take().expect("the radio's parts are used once");
        esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 66320);
        let timg0 = TimerGroup::new(parts.timer);
        esp_rtos::start(timg0.timer0, parts.interrupt);
        let controller =
            WifiController::new(parts.wifi, Default::default()).expect("Wi-Fi controller");
        self.radio = Some(Radio { controller });
    }

    /// The radio, started if it was not yet.
    pub fn get(&mut self) -> &mut Radio {
        self.start();
        self.radio.as_mut().expect("started above")
    }

    /// The radio by value, for a job that never gives it back.
    pub fn take(mut self) -> Radio {
        self.start();
        self.radio.take().expect("started above")
    }
}
