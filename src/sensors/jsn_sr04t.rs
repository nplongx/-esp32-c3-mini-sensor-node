use embedded_hal::delay::DelayNs;
use esp_idf_hal::delay::Ets;
use esp_idf_hal::gpio::{Input, Output, PinDriver};
use esp_idf_hal::sys::esp_timer_get_time; // Lấy hàm từ module sys của hal thay vì gọi esp_idf_sys ngoài

// Tốc độ âm thanh trong không khí là ~343 m/s tương đương 0.0343 cm/us
const SOUND_SPEED_CM_PER_US: f32 = 0.0343;

// Khoảng cách tối đa muốn đo (ví dụ: 400 cm).
// Tính toán timeout: (400cm * 2) / 0.0343 = ~23323 us (Khoảng 23ms).
// Đặt timeout 30ms (30,000 us) là an toàn.
const TIMEOUT_US: i64 = 30_000;

/// Struct quản lý cảm biến siêu âm JSN-SR04T
pub struct HydroponicLevelSensor<'a> {
    trig_pin: PinDriver<'a, Output>,
    echo_pin: PinDriver<'a, Input>,
}

impl<'a> HydroponicLevelSensor<'a> {
    /// Khởi tạo module siêu âm với chân Trigger và Echo
    pub fn new(
        mut trig_pin: PinDriver<'a, Output>,
        echo_pin: PinDriver<'a, Input>,
    ) -> anyhow::Result<Self> {
        // Đảm bảo chân Trig ở mức Low khi khởi động
        trig_pin.set_low()?;
        Ok(Self { trig_pin, echo_pin })
    }

    /// Trả về khoảng cách từ cảm biến đến mặt nước (tính bằng cm)
    pub fn read_distance(&mut self) -> anyhow::Result<Option<f32>> {
        // 1. Tạo xung Trigger 10 micro-giây để đánh thức cảm biến
        self.trig_pin.set_low()?;

        // Khởi tạo delay để tạo xung. (Ở bản mới Ets cần tạo instance)
        let mut delay = Ets;

        delay.delay_us(2); // Delay ngắn để đảm bảo tín hiệu sạch
        self.trig_pin.set_high()?;
        delay.delay_us(10); // Xung HIGH ít nhất 10us theo chuẩn datasheet
        self.trig_pin.set_low()?;

        // 2. Chờ chân Echo lên mức HIGH (Bắt đầu phát sóng)
        let start_wait = unsafe { esp_timer_get_time() };
        while self.echo_pin.is_low() {
            if unsafe { esp_timer_get_time() } - start_wait > TIMEOUT_US {
                log::warn!("Siêu âm Timeout: Chân Echo không phản hồi (kẹt Low).");
                return Ok(None);
            }
        }

        // 3. Ghi nhận thời điểm bắt đầu nhận sóng phản hồi
        let start_time = unsafe { esp_timer_get_time() };

        // 4. Chờ chân Echo xuống mức LOW (Nhận được sóng phản hồi)
        while self.echo_pin.is_high() {
            if unsafe { esp_timer_get_time() } - start_time > TIMEOUT_US {
                log::warn!(
                    "Siêu âm Timeout: Không nhận được sóng phản hồi (ngoài tầm đo hoặc kẹt High)."
                );
                return Ok(None);
            }
        }

        // 5. Tính toán thời gian phản hồi
        let end_time = unsafe { esp_timer_get_time() };
        let duration_us = end_time - start_time;

        // 6. Chuyển đổi thời gian sang khoảng cách (cm)
        // Khoảng cách = (Thời gian * Tốc độ âm thanh) / 2 (Vì sóng đi cả chiều đi và về)
        let distance_cm = (duration_us as f32 * SOUND_SPEED_CM_PER_US) / 2.0;

        // Bỏ qua các giá trị rác (quá nhỏ do nhiễu hoặc điểm mù)
        // JSN-SR04T thường có điểm mù là 20cm (HC-SR04 thì khoảng 2cm)
        if distance_cm < 20.0 {
            log::debug!(
                "Khoảng cách đo được ({:.1} cm) nằm trong điểm mù (dead zone).",
                distance_cm
            );
            return Ok(None);
        }

        Ok(Some(distance_cm))
    }
}
