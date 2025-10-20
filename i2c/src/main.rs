#![no_std]
#![no_main]

use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_nrf::bind_interrupts;
use embassy_nrf::peripherals::TWISPI0;
use embassy_nrf::{
    gpio::{Level, Output, OutputDrive},
    peripherals, rng,
    twim::{self, Twim},
};
use embassy_time::{Delay, Timer};
use static_cell::ConstStaticCell;
use {defmt_rtt as _, panic_probe as _};

use heapless::String;
use libm::{fabsf, roundf, truncf};
use max7219::MAX7219;

bind_interrupts!(struct Irqs {
    RNG => rng::InterruptHandler<peripherals::RNG>;
    EGU0_SWI0 => nrf_sdc::mpsl::LowPrioInterruptHandler;
    CLOCK_POWER => nrf_sdc::mpsl::ClockInterruptHandler;
    RADIO => nrf_sdc::mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
    TWISPI0 => twim::InterruptHandler<TWISPI0>;
});

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

    loop {
        let measurements = bme.measure(&mut Delay).expect("to measure temperature");

        defmt::info!("Relative Humidity = {}%", measurements.humidity);
        defmt::info!("Temperature = {} deg C", measurements.temperature);
        defmt::info!("Pressure = {} pascals", measurements.pressure);

        let int_humidity = truncf(measurements.humidity) as i32;
        let temp = measurements.temperature;
        let int_temp = truncf(temp) as i32;
        let frac_temp = (roundf(fabsf(temp - truncf(temp)) * 10.0)) as u32;

        let mut s = String::<16>::new();
        write!(&mut s, "{:02}{}C{:03}H", int_temp, frac_temp, int_humidity).unwrap();
        defmt::info!("{}", s.as_str());

        let buf: [u8; 8] = s.as_bytes()[..8].try_into().unwrap();
        driver.write_str(0, &buf, 0b01000000).unwrap();
        Timer::after_secs(5).await;
    }
}
