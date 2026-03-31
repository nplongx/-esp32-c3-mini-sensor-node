use crate::models::{CommandPayload, SensorNodeConfig, SharedConfig, SystemEvent};
// Ghi chú: Có thể bỏ SensorData nếu không còn dùng ở đâu khác
use esp_idf_svc::mqtt::client::{EspMqttClient, EventPayload, MqttClientConfiguration, QoS};
use std::sync::mpsc::Sender;

pub struct HydroponicMqtt<'a> {
    client: EspMqttClient<'a>,
    base_topic: String,
    config_topic: String,
    cmd_topic: String,
}

impl<'a> HydroponicMqtt<'a> {
    /// Cấu hình và khởi tạo kết nối MQTT đến Broker
    pub fn new(
        broker_url: &str,
        client_id: &str,
        shared_config: SharedConfig,
        conn_tx: Sender<SystemEvent>,
    ) -> anyhow::Result<Self> {
        let mut config = MqttClientConfiguration::default();
        config.client_id = Some(client_id);

        let config_topic = format!("AGITECH/{}/config/sensor_node", client_id);
        let cmd_topic = format!("AGITECH/{}/sensor/command", client_id);

        let config_topic_clone = config_topic.clone();
        let cmd_topic_clone = cmd_topic.clone();

        let client = EspMqttClient::new_cb(broker_url, &config, move |event| {
            match event.payload() {
                EventPayload::Connected(_) => {
                    log::info!("✅ [MQTT] Đã kết nối thành công đến Broker!");
                    let _ = conn_tx.send(SystemEvent::MqttConnected);
                }
                EventPayload::Disconnected => {
                    log::warn!("⚠️ [MQTT] Mất kết nối, đang thử lại...");
                    let _ = conn_tx.send(SystemEvent::MqttDisconnected);
                }
                EventPayload::Published(id) => {
                    log::debug!("[MQTT] Đã gửi bản tin thành công (ID: {})", id)
                }
                EventPayload::Received { topic, data, .. } => {
                    let topic_str = topic.unwrap_or("");

                    // ĐÃ SỬA LỖI: Bỏ vòng if bao bọc bên ngoài để check song song 2 topic
                    if topic_str == config_topic_clone.as_str() {
                        log::info!("[MQTT] Nhận được bản tin cấu hình mới, đang xử lý...");
                        if let Ok(new_config) = serde_json::from_slice::<SensorNodeConfig>(data) {
                            if let Ok(mut lock) = shared_config.write() {
                                *lock = new_config;
                                log::info!("[MQTT] Đã cập nhật cấu hình runtime!");
                            }
                        }
                    } else if topic_str == cmd_topic_clone.as_str() {
                        log::info!("[MQTT] Nhận được bản tin lệnh (command)...");
                        if let Ok(payload) = serde_json::from_slice::<CommandPayload>(data) {
                            if payload.command == "continuous_level" {
                                log::info!("🚀 Nhận lệnh continuous_level: {}", payload.state);
                                let _ =
                                    conn_tx.send(SystemEvent::SetContinuousLevel(payload.state));
                            }
                        }
                    }
                }
                _ => {}
            }
        })?;

        log::info!("[MQTT] Client đã được khởi tạo, đang chờ kết nối...");

        Ok(Self {
            client,
            // Đổi base_topic linh hoạt theo client_id thay vì fix cứng
            base_topic: format!("AGITECH/{}/sensor", client_id),
            config_topic,
            cmd_topic,
        })
    }

    pub fn subscribe_topics(&mut self) {
        // 1. Subscribe Topic Cấu Hình
        match self.client.subscribe(&self.config_topic, QoS::AtLeastOnce) {
            Ok(_) => log::info!("✅ [MQTT] Đã subscribe config: {}", self.config_topic),
            Err(e) => log::error!("❌ [MQTT] Lỗi subscribe config: {:?}", e),
        }

        // 2. ĐÃ SỬA LỖI: Thêm Subscribe Topic Lệnh (Command)
        match self.client.subscribe(&self.cmd_topic, QoS::AtLeastOnce) {
            Ok(_) => log::info!("✅ [MQTT] Đã subscribe command: {}", self.cmd_topic),
            Err(e) => log::error!("❌ [MQTT] Lỗi subscribe command: {:?}", e),
        }
    }

    /// Đẩy 1 cục JSON tổng hợp
    /// `topic_suffix`: hậu tố của topic (vd: "data", "status")
    pub fn publish_raw_payload(&mut self, topic_suffix: &str, payload: &str) -> anyhow::Result<()> {
        // Tự động nối với base topic. Ví dụ: AGITECH/device_001/sensor/data
        let topic = format!("{}/{}", self.base_topic, topic_suffix);

        self.client
            .publish(&topic, QoS::AtLeastOnce, false, payload.as_bytes())?;

        log::info!("Gửi MQTT -> [{}]: {}", topic, payload);

        Ok(())
    }
}
