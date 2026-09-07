use embassy_executor::Spawner;
use embassy_net::{Stack, StackResources};
use embassy_time::{Duration, Timer};
use esp_hal::{
    peripherals::{Peripherals, WIFI},
    rng::Rng,
};
use esp_println::println;
use esp_radio::wifi::{Config, ControllerConfig, WifiController, sta::StationConfig};
use static_cell::StaticCell;

const SSID: &str = env!("WIFI_SSID");
const PWD: &str = env!("WIFI_PWD");

pub async fn init_wifi(spawner: Spawner, wifi: WIFI<'static>) -> Stack<'static> {
    let wifi_conf = Config::Station(
        StationConfig::default()
            .with_ssid(SSID)
            .with_password(PWD.into()),
    );

    let (controller, interfaces) = esp_radio::wifi::new(
        wifi,
        ControllerConfig::default().with_initial_config(wifi_conf),
    )
    .expect("Failed to initialize Wi-Fi controller");

    let wifi_interface = interfaces.station;
    let config = embassy_net::Config::dhcpv4(Default::default());
    let rng = Rng::new();
    let seed = (rng.random() as u64) << 32 | rng.random() as u64;

    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        wifi_interface,
        config,
        RESOURCES.init(StackResources::new()),
        seed,
    );

    spawner.spawn(net_task(runner).unwrap());
    spawner.spawn(connection(controller).unwrap());

    stack.wait_config_up().await;
    let addr = stack.config_v4().unwrap().address;

    println!("My addr: {}", addr.address());

    stack
}

#[embassy_executor::task]
async fn net_task(
    mut runner: embassy_net::Runner<'static, esp_radio::wifi::Interface<'static>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) -> ! {
    loop {
        match controller.connect_async().await {
            Ok(info) => {
                println!("Wifi connected to {:?}", info);

                let info = controller.wait_for_disconnect_async().await.ok();
                println!("Disconnected: {:?}", info);
            }
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
            }
        }

        Timer::after(Duration::from_secs(5)).await;
    }
}
