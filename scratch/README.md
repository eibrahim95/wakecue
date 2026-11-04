# scratch

Throwaway firmware that proved one thing at a time on the real board. None of
it is the lamp; the lamp crate is `firmware_ws/wakecue/`.

| Project | What it proved |
| --- | --- |
| `blink-test/` | The board flashes, the onboard LED is active-low on GPIO8, and LEDC PWM works. |
| `wifi-ntp/` | Wi-Fi, DHCP, DNS and NTP; the DS3231 on I²C at 3.3 V (time written and read back, Alarm 1 pulling a pin low); deep sleep woken by GPIO3 low or GPIO4 high; the clock's `OSF` flag read, cleared and used to decide whether the radio runs at all. |

`wifi-ntp` is the one still in use. Run `just` inside it to build and flash;
the Wi-Fi credentials come from the untracked `.env` beside the Justfile, and
`just --list` shows the other recipes.
