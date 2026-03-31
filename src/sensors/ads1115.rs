use ads1x1x::{
    channel,
    ic::{Ads1115, Resolution16Bit},
    mode::OneShot,
    Ads1x1x, Error as AdsError, FullScaleRange, TargetAddr,
};
// embedded_hal 1.0 uses the i2c module directly
use embedded_hal::i2c::I2c;

/// Struct quản lý một module ADS1115
pub struct HydroponicAds1115<I2C> {
    adc: Ads1x1x<I2C, Ads1115, Resolution16Bit, OneShot>,
    v_ref_expected: f32,
    fsr: FullScaleRange,
}

impl<I2C, E> HydroponicAds1115<I2C>
where
    I2C: I2c<Error = E>, // Sử dụng trait I2c tổng quát của HAL 1.0
    E: core::fmt::Debug,
{
    /// Khởi tạo ADS1115
    pub fn new(i2c: I2C, address: TargetAddr, v_ref_expected: f32) -> Result<Self, AdsError<E>> {
        let mut adc = Ads1x1x::new_ads1115(i2c, address);
        let fsr = FullScaleRange::Within6_144V;

        adc.set_full_scale_range(fsr)?;

        Ok(Self {
            adc,
            v_ref_expected,
            fsr,
        })
    }

    fn get_voltage_multiplier(&self) -> f32 {
        match self.fsr {
            FullScaleRange::Within6_144V => 0.0001875,
            FullScaleRange::Within4_096V => 0.000125,
            FullScaleRange::Within2_048V => 0.0000625,
            FullScaleRange::Within1_024V => 0.00003125,
            FullScaleRange::Within0_512V => 0.000015625,
            FullScaleRange::Within0_256V => 0.0000078125,
        }
    }

    fn read_voltage_single_a3(&mut self) -> Result<f32, AdsError<E>> {
        // Wrap the read in nb::block!
        let raw_value = nb::block!(self.adc.read(channel::SingleA3))?;
        Ok((raw_value as f32) * self.get_voltage_multiplier())
    }

    fn read_voltage_diff_a0_a1(&mut self) -> Result<f32, AdsError<E>> {
        // Wrap the read in nb::block!
        let raw_value = nb::block!(self.adc.read(channel::DifferentialA0A1))?;
        Ok((raw_value as f32) * self.get_voltage_multiplier())
    }

    pub fn read_calibrated_signal(&mut self) -> Result<f32, AdsError<E>> {
        let v_a3_actual = self.read_voltage_single_a3()?;

        if v_a3_actual <= 0.01 {
            return Ok(0.0);
        }

        let v_raw_diff = self.read_voltage_diff_a0_a1()?;
        let correction_factor = self.v_ref_expected / v_a3_actual;
        let v_calibrated = v_raw_diff * correction_factor;

        Ok(v_calibrated)
    }

    pub fn release(self) -> I2C {
        self.adc.destroy_ads1115()
    }
}
