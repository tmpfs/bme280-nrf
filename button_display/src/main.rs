#![no_std]
#![no_main]

use bme280::i2c::BME280;
use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_futures::select::Either;
use embassy_futures::{join::join, select::select};
use embassy_nrf::bind_interrupts;
use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::{
    gpio::{Level, Output, OutputDrive},
    peripherals,
    twim::{self, Twim},
};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Delay, Timer};
use heapless::String;
use libm::{fabsf, roundf, truncf};
use max7219::MAX7219;
use max7219::connectors::PinConnector;
use static_cell::ConstStaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    TWISPI0 => twim::InterruptHandler<peripherals::TWISPI0>;
});

#[repr(u8)]
#[derive(Default, Copy, Clone, defmt::Format)]
enum DisplayState {
    #[default]
    Temp = 1,
    Humidity = 2,
    Pressure = 3,
}

impl DisplayState {
    fn next_state(&self) -> Self {
        match self {
            Self::Temp => Self::Humidity,
            Self::Humidity => Self::Pressure,
            Self::Pressure => Self::Temp,
        }
    }
}

static CYCLE_DISPLAY: Signal<CriticalSectionRawMutex, DisplayState> = Signal::new();

async fn wait_for_pull_up(mut input: Input<'static>, mut state: DisplayState) -> ! {
    loop {
        input.wait_for_rising_edge().await;
        state = state.next_state();
        CYCLE_DISPLAY.signal(state);
        // Debounce a little
        Timer::after_millis(50).await;
    }
}

async fn refresh_display(
    mut bme: BME280<Twim<'static>>,
    mut display: MAX7219<PinConnector<Output<'static>, Output<'static>, Output<'static>>>,
    mut current_state: DisplayState,
) -> ! {
    loop {
        match select(CYCLE_DISPLAY.wait(), Timer::after_millis(30)).await {
            Either::First(next_state) => {
                defmt::info!("state = {}", next_state);
                current_state = next_state;
            }
            Either::Second(_) => match bme.measure(&mut Delay) {
                Ok(measurements) => {
                    defmt::debug!("Relative Humidity = {}%", measurements.humidity);
                    defmt::debug!("Temperature = {} deg C", measurements.temperature);
                    defmt::debug!("Pressure = {} pascals", measurements.pressure);

                    let (value, dots) = match current_state {
                        DisplayState::Temp => {
                            let temp = measurements.temperature;
                            // let temp = -39.45f32;
                            // let temp = -2.74f32;
                            let int_temp = truncf(temp) as i32;
                            let int_temp = int_temp.min(85);
                            let int_temp = int_temp.max(-40);
                            let frac_temp = (roundf(fabsf(temp - truncf(temp)) * 10.0)) as u32;
                            let frac_temp = frac_temp.min(9);
                            let mut s = String::<8>::new();
                            write!(&mut s, "{:>3}{}{:>4}", int_temp, frac_temp, "CEL").unwrap();
                            (s, 0b00100000)
                        }
                        DisplayState::Humidity => {
                            let humidity = truncf(measurements.humidity) as i32;
                            let mut s = String::<8>::new();
                            write!(&mut s, "{:>4}{:>4}", humidity, "PHU").unwrap();
                            (s, 0)
                        }
                        DisplayState::Pressure => {
                            let pressure = truncf(measurements.pressure) as i32 / 100;
                            let mut s = String::<8>::new();
                            write!(&mut s, "{:>4}{:>4}", pressure, "HPA").unwrap();
                            (s, 0)
                        }
                    };

                    defmt::debug!("{}", value.as_str());
                    let buf: [u8; 8] = value.as_bytes().try_into().unwrap();
                    display.write_str(0, &buf, dots).unwrap();
                }
                Err(_) => {
                    defmt::warn!("failed to measure BME280 sensor");
                }
            },
        }
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    let sck = p.P0_14;
    let mosi = p.P0_15;
    let cs_pin = p.P0_16;

    let cs = Output::new(cs_pin, Level::High, OutputDrive::HighDrive);
    let sck = Output::new(sck, Level::High, OutputDrive::HighDrive);
    let mosi = Output::new(mosi, Level::High, OutputDrive::HighDrive);

    let mut driver = MAX7219::from_pins(1, mosi, cs, sck).expect("to init MAX7219");
    driver.power_on().unwrap();
    driver.set_intensity(0, 0x1).unwrap();

    defmt::info!("max7219 display initialised!");

    let sda = p.P0_13;
    let scl = p.P0_12;

    // Create I2C instance
    static RAM_BUFFER: ConstStaticCell<[u8; 16]> = ConstStaticCell::new([0; 16]);
    let i2c = Twim::new(
        p.TWISPI0,
        Irqs,
        sda,
        scl,
        twim::Config::default(),
        RAM_BUFFER.take(),
    );

    let mut bme = bme280::i2c::BME280::new_primary(i2c);
    bme.init(&mut Delay).expect("to init bme280 sensor");

    let input = Input::new(p.P1_05, Pull::Up);
    let default_state = DisplayState::default();
    join(
        wait_for_pull_up(input, default_state),
        refresh_display(bme, driver, default_state),
    )
    .await;
}
