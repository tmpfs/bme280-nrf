#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_nrf::bind_interrupts;
use embassy_nrf::peripherals::TWISPI0;
use embassy_nrf::{
    peripherals, rng,
    twim::{self, Twim},
};
use embassy_time::{Delay, Timer};
use static_cell::ConstStaticCell;
use {defmt_rtt as _, panic_probe as _};

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
        Timer::after_secs(5).await;
    }
}
