use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;

// Tạm thời comment các thư viện I2C
// use embedded_hal_bus::i2c::RefCellDevice;
use esp_idf_hal::gpio::{PinDriver, Pull};
// use esp_idf_hal::i2c::{I2cConfig, I2cDriver};
use esp_idf_hal::peripherals::Peripherals;
// use esp_idf_hal::units::FromValueType;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::nvs::EspDefaultNvsPartition;

// --- Khai báo các module con trong project ---
mod models;
mod network;
mod sensors;

// 🟢 SỬA Ở ĐÂY: Đổi AppConfig thành SensorNodeConfig
use crate::models::{SensorNodeConfig, SharedConfig, SystemEvent};
use crate::network::mqtt::HydroponicMqtt;
use crate::network::wifi::HydroponicNetwork;
// use crate::sensors::ads1115::HydroponicAds1115;
use crate::sensors::ds18b20::HydroponicTempSensor;
use crate::sensors::jsn_sr04t::HydroponicLevelSensor;

// --- THUẬT TOÁN ĐIỀU KHIỂN ---
fn convert_voltage_to_ph(voltage: f32, config: &SensorNodeConfig) -> f32 {
    let slope = (config.ph_v7 - config.ph_v4) / 3.0;
    if slope.abs() < 0.001 {
        return 7.0;
    }
    let ph_current = 7.0 + (voltage - config.ph_v7) / slope;
    ph_current.clamp(0.0, 14.0)
}

fn convert_voltage_to_ec(voltage: f32, temperature_c: f32, config: &SensorNodeConfig) -> f32 {
    if voltage <= 0.01 {
        return 0.0;
    }
    let ec_raw_us = (voltage * config.ec_factor) + config.ec_offset;

    let real_temp = temperature_c + config.temp_offset;
    let temp_coefficient = 1.0 + config.temp_compensation_beta * (real_temp - 25.0);

    let ec_ms = (ec_raw_us / temp_coefficient) / 1000.0;
    ec_ms.clamp(0.0, 4.4)
}

fn main() -> anyhow::Result<()> {
    // 1. Khởi tạo ESP-IDF và Logger
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();
    log::info!("Khởi động Firmware Giám sát Thủy canh AgiTech (ESP32-C3)!");

    let peripherals = Peripherals::take().unwrap();
    let pins = peripherals.pins;

    // 2. Khởi tạo dịch vụ Hệ thống (NVS, EventLoop)
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    // 3. Kết nối Mạng
    let _network_manager =
        HydroponicNetwork::connect(peripherals.modem, sys_loop, nvs, "Huynh Hong", "123443215")?;

    let shared_config: SharedConfig = Arc::new(RwLock::new(SensorNodeConfig::default()));
    let mqtt_config_clone = shared_config.clone();

    // 4. Khởi tạo MQTT Broker
    let broker_url = "mqtt://192.168.1.6:1883";
    let client_id = "device_001";
    let (conn_tx, conn_rx) = mpsc::channel::<SystemEvent>();

    let mut mqtt_client = HydroponicMqtt::new(broker_url, client_id, mqtt_config_clone, conn_tx)?;
    let mut is_mqtt_connected = false;

    // ---------------------------------------------------------
    // 5. KHỞI TẠO PHẦN CỨNG & CẢM BIẾN
    // ---------------------------------------------------------

    // ==== [TẠM TẮT] Khởi tạo I2C và ADS1115 (EC, pH) ====
    /*
    let i2c_config = I2cConfig::new().baudrate(100_u32.kHz().into());
    let i2c_driver = I2cDriver::new(peripherals.i2c0, pins.gpio8, pins.gpio9, &i2c_config)?;
    let i2c_bus = RefCell::new(i2c_driver);

    let mut ec_adc = HydroponicAds1115::new(RefCellDevice::new(&i2c_bus), ads1x1x::TargetAddr::default(), 5.0).unwrap();
    let mut ph_adc = HydroponicAds1115::new(RefCellDevice::new(&i2c_bus), ads1x1x::TargetAddr::Vdd, 5.0).unwrap();
    */
    // ===================================================

    let ow_pin = PinDriver::input_output(pins.gpio5, Pull::Up)?;
    let mut temp_sensor = HydroponicTempSensor::new(ow_pin)?;

    let trig_pin = PinDriver::output(pins.gpio7)?;
    let echo_pin = PinDriver::input(pins.gpio10, Pull::Floating)?;
    let mut level_sensor = HydroponicLevelSensor::new(trig_pin, echo_pin)?;

    // ---------------------------------------------------------
    // 6. VÒNG LẶP CHÍNH
    // ---------------------------------------------------------
    log::info!("Hoàn tất thiết lập phần cứng (Tạm bỏ qua EC/pH). Bắt đầu thu thập dữ liệu...");

    let mut last_periodic_time = std::time::Instant::now();
    let mut last_continuous_time = std::time::Instant::now();
    let mut is_continuous_level_mode = false;

    let continuous_interval = Duration::from_millis(500); // 500ms gửi siêu âm 1 lần khi có lệnh

    loop {
        // 6.1 Lắng nghe Event từ MQTT Channel
        if let Ok(event) = conn_rx.try_recv() {
            match event {
                SystemEvent::MqttConnected => {
                    is_mqtt_connected = true;
                    mqtt_client.subscribe_topics();
                }
                SystemEvent::MqttDisconnected => {
                    is_mqtt_connected = false;
                }
                SystemEvent::SetContinuousLevel(state) => {
                    is_continuous_level_mode = state;
                    if state {
                        log::warn!("🚀 Kích hoạt chế độ đọc mực nước LIÊN TỤC!");
                    } else {
                        log::info!("🛑 Trở về chế độ đọc định kỳ.");
                    }
                }
            }
        }

        // Lấy config hiện tại để sử dụng cho vòng lặp này
        let current_config = { shared_config.read().unwrap().clone() };

        // periodic_interval liên tục theo cấu hình mới nhất (đơn vị ms)
        let periodic_interval = Duration::from_millis(current_config.publish_interval as u64);

        // 6.2 TASK 1: Đọc và gửi ĐỊNH KỲ
        if last_periodic_time.elapsed() >= periodic_interval {
            log::info!("--- Gửi báo cáo định kỳ ---");

            let mut current_temp = 25.0;
            let mut ec_value = 0.0;
            let mut ph_value = 7.0;
            let mut distance = 0.0;

            // Đọc các cảm biến (Giả sử các tính năng đều enable)
            if current_config.is_temp_enabled {
                current_temp = temp_sensor
                    .read_temperature()
                    .unwrap_or(Some(25.0))
                    .unwrap_or(25.0);
            }
            if current_config.is_ec_enabled {
                // v_ec = ...; ec_value = convert_voltage_to_ec(...);
            }
            if current_config.is_ph_enabled {
                // v_ph = ...; ph_value = convert_voltage_to_ph(...);
            }
            if !is_continuous_level_mode && current_config.is_water_level_enabled {
                if let Ok(Some(dist)) = level_sensor.read_distance() {
                    distance = dist;
                }
            }

            // Lấy timestamp hiện tại (nếu board có sync thời gian, hoặc dùng millis từ lúc boot)
            let current_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();

            // Gom thành 1 chuỗi JSON
            if is_mqtt_connected {
                let payload = format!(
                    r#"{{"temp":{:.1}, "ec":{:.2}, "ph":{:.2}, "water_level":{:.1}, "timestamp_ms":{}}}"#,
                    current_temp, ec_value, ph_value, distance, current_ms
                );

                // Cần sửa lại hàm publish của MQTT Client để nhận chuỗi thay vì 3 tham số lẻ
                let _ = mqtt_client.publish_raw_payload("sensor/data", &payload);
            }

            last_periodic_time = std::time::Instant::now();
        }

        // 6.3 TASK 2: Đọc mực nước LIÊN TỤC (Chỉ chạy khi có lệnh từ Controller VÀ tính năng được bật)
        if is_continuous_level_mode
            && current_config.is_water_level_enabled
            && last_continuous_time.elapsed() >= continuous_interval
        {
            match level_sensor.read_distance() {
                Ok(Some(distance)) => {
                    if is_mqtt_connected {
                        let current_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis();
                        let payload = format!(
                            r#"{{"water_level": {:.1}, "timestamp_ms":{}}}"#,
                            distance, current_ms
                        );
                        let _ = mqtt_client.publish_raw_payload("sensor/data", &payload);
                    }
                }
                Ok(None) => log::warn!("Ngoài tầm siêu âm"),
                Err(_) => log::error!("Lỗi JSN-SR04T"),
            }
            last_continuous_time = std::time::Instant::now();
        }

        // Nhường CPU 50ms (Chạy mượt hơn)
        thread::sleep(Duration::from_millis(50));
    }
}
