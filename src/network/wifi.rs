use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::modem::Modem;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sntp::{EspSntp, SyncStatus};
use esp_idf_svc::wifi::BlockingWifi;
use esp_idf_svc::wifi::{AuthMethod, ClientConfiguration, Configuration, EspWifi};
use std::thread;
use std::time::Duration;

/// Struct quản lý kết nối Mạng và Thời gian
pub struct HydroponicNetwork<'a> {
    _wifi: BlockingWifi<EspWifi<'a>>,
    _sntp: EspSntp<'a>, // Giữ instance này để ESP32 liên tục đồng bộ giờ ngầm
}

impl<'a> HydroponicNetwork<'a> {
    /// Khởi tạo WiFi, kết nối và đồng bộ thời gian SNTP
    pub fn connect(
        modem: Modem<'a>,
        sys_loop: EspSystemEventLoop,
        nvs: EspDefaultNvsPartition,
        ssid: &str,
        password: &str,
    ) -> anyhow::Result<Self> {
        // 1. Khởi tạo driver WiFi
        let wifi = EspWifi::new(modem, sys_loop.clone(), Some(nvs))?;
        let mut wifi = BlockingWifi::wrap(wifi, sys_loop)?;

        // 2. Cấu hình chế độ Station (Client)
        let wifi_config = Configuration::Client(ClientConfiguration {
            ssid: ssid.try_into().unwrap(),
            password: password.try_into().unwrap(),
            auth_method: AuthMethod::WPA2Personal,
            ..Default::default()
        });

        wifi.set_configuration(&wifi_config)?;

        // 3. Khởi động và kết nối
        log::info!("Đang khởi động WiFi...");
        wifi.start()?;

        log::info!("Đang kết nối đến SSID: {}", ssid);
        wifi.connect()?;

        // Chờ cho đến khi được cấp phát IP từ Router
        wifi.wait_netif_up()?;

        let ip_info = wifi.wifi().sta_netif().get_ip_info()?;
        log::info!("Đã kết nối WiFi thành công! IP: {}", ip_info.ip);

        // 4. Khởi tạo và đồng bộ thời gian (SNTP)
        log::info!("Đang đồng bộ thời gian thực (SNTP)...");
        let sntp = EspSntp::new_default()?;

        // Vòng lặp chờ cho đến khi thời gian được đồng bộ xong
        while sntp.get_sync_status() != SyncStatus::Completed {
            thread::sleep(Duration::from_millis(500));
        }
        log::info!("Đồng bộ thời gian thành công!");

        Ok(Self {
            _wifi: wifi,
            _sntp: sntp,
        })
    }
}
