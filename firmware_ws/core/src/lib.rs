//! Lamp & Alarm logic that runs on the PC and on the chip: no hardware, no allocation.
#![no_std]

pub mod astro;
pub mod buttons;
pub mod calendar;
pub mod dfplayer;
pub mod dhcp;
pub mod dns;
pub mod ds3231;
pub mod http;
pub mod ina219;
pub mod menu;
pub mod portal;
pub mod ramp;
pub mod schedule;
pub mod settings;
pub mod setup_page;
