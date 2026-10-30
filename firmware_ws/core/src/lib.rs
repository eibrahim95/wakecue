//! Lamp & Alarm logic that runs on the PC and on the chip: no hardware, no allocation.
#![no_std]

pub mod buttons;
pub mod calendar;
pub mod dfplayer;
pub mod ds3231;
pub mod ina219;
pub mod menu;
pub mod ramp;
pub mod schedule;
pub mod settings;
