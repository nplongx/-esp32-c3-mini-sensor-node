use std::sync::{Arc, RwLock};

use esp_idf_hal::sys::bintime;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct SensorData {
    pub value: f32,
    pub unit: String,
    pub timestamp: u64, // Unix Epoch Time (giây)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorNodeConfig {
    pub device_id: String,

    // --- HIỆU CHUẨN ---
    pub ph_v7: f32,
    pub ph_v4: f32,
    pub ec_factor: f32,
    pub ec_offset: f32, // Thêm mới từ DB
    pub temp_offset: f32,
    pub temp_compensation_beta: f32, // Thêm mới từ DB

    // --- LỌC NHIỄU & TẦN SUẤT ---
    pub sampling_interval: i64, // Đổi sang i64 cho chuẩn SQLite INTEGER
    pub publish_interval: i64,  // Thêm mới từ DB
    pub moving_average_window: i64, // Thêm mới từ DB

    // --- TRẠNG THÁI (SQLite lưu là INTEGER 0 hoặc 1) ---
    pub is_ph_enabled: bool,   // Thêm mới từ DB
    pub is_ec_enabled: bool,   // Thêm mới từ DB
    pub is_temp_enabled: bool, // Thêm mới từ DB
    pub is_water_level_enabled: bool,
}

impl Default for SensorNodeConfig {
    fn default() -> Self {
        Self {
            device_id: String::new(),
            ph_v7: 2.5,
            ph_v4: 3.0,
            ec_factor: 880.0,
            ec_offset: 0.0,
            temp_offset: 0.0,
            temp_compensation_beta: 0.02,
            sampling_interval: 1000,
            publish_interval: 5000,
            moving_average_window: 10,
            is_ph_enabled: true,
            is_ec_enabled: true,
            is_temp_enabled: true,
            is_water_level_enabled: true,
        }
    }
}

// Định nghĩa kiểu dữ liệu dùng chung (Thread-safe Shared State)
pub type SharedConfig = Arc<RwLock<SensorNodeConfig>>;

#[derive(Debug, Clone)]
pub enum SystemEvent {
    MqttConnected,
    MqttDisconnected,
    SetContinuousLevel(bool), // true: bật đọc liên tục, false: tắt
}

// Struct để parse lệnh từ Controller gửi xuống
#[derive(Debug, Deserialize)]
pub struct CommandPayload {
    pub command: String,
    pub state: bool,
}
