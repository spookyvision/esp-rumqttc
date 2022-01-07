use std::{
    convert::TryInto, io, io::prelude::*, net::TcpStream, str::FromStr, sync::Arc, time::Duration,
};

use anyhow::{anyhow, bail};
use embedded_svc::{
    ipv4,
    ping::Ping,
    wifi::{
        AccessPointConfiguration, ApIpStatus, ApStatus, ClientConfiguration,
        ClientConnectionStatus, ClientIpStatus, ClientStatus, Configuration, Status, Wifi,
    },
};
use esp_idf_hal::prelude::*;
use esp_idf_svc::{
    netif::EspNetifStack, nvs::EspDefaultNvs, ping, sysloop::EspSysLoopStack, wifi::EspWifi,
};
use esp_idf_sys as _;
use framed::Network;
use log::info;
use mqttbytes::v4::{Connect, ConnectReturnCode, Login, Packet, Publish};
use rumq::{Incoming, MqttOptions};
use url::Url;

use crate::{eventloop::EventLoop, rumq::Request}; // If using the `binstart` feature of `esp-idf-sys`, always keep this module imported

mod eventloop;
mod framed;
mod rumq;
mod state;

// const SSID: &str = "SSID";
// const PASS: &str = "PSK";
const MQTT_USER: &str = "horse";
const MQTT_PASS: &str = "CorrectHorseBatteryStaple";

fn test_tcp() -> anyhow::Result<()> {
    info!("About to open a TCP connection to 1.1.1.1 port 80");

    let mut stream = TcpStream::connect("one.one.one.one:80")?;

    let err = stream.try_clone();
    if let Err(err) = err {
        info!(
            "Duplication of file descriptors does not work (yet) on the ESP-IDF, as expected: {}",
            err
        );
    }

    stream.write_all("GET / HTTP/1.0\n\n".as_bytes())?;

    let mut result = Vec::new();

    stream.read_to_end(&mut result)?;

    info!(
        "1.1.1.1 returned:\n=================\n{}\n=================\nSince it returned something, all is OK",
        std::str::from_utf8(&result)?);

    Ok(())
}

fn wifi(
    netif_stack: Arc<EspNetifStack>,
    sys_loop_stack: Arc<EspSysLoopStack>,
    default_nvs: Arc<EspDefaultNvs>,
) -> anyhow::Result<Box<EspWifi>> {
    let ssid: &str = option_env!("SSID").ok_or(anyhow!("no SSID"))?;
    let psk: &str = option_env!("PSK").ok_or(anyhow!("no PSK"))?;

    let mut wifi = Box::new(EspWifi::new(netif_stack, sys_loop_stack, default_nvs)?);

    info!("Wifi created, about to scan");

    let ap_infos = wifi.scan()?;

    let ours = ap_infos.into_iter().find(|a| a.ssid == ssid);

    let channel = if let Some(ours) = ours {
        info!(
            "Found configured access point {} on channel {}",
            ssid, ours.channel
        );
        Some(ours.channel)
    } else {
        info!(
            "Configured access point {} not found during scanning, will go with unknown channel",
            ssid
        );
        None
    };

    wifi.set_configuration(&Configuration::Mixed(
        ClientConfiguration {
            ssid: ssid.into(),
            password: psk.into(),
            channel,
            ..Default::default()
        },
        AccessPointConfiguration {
            ssid: "aptest".into(),
            channel: channel.unwrap_or(1),
            ..Default::default()
        },
    ))?;

    info!("Wifi configuration set, about to get status");

    let status = wifi.get_status();

    if let Status(
        ClientStatus::Started(ClientConnectionStatus::Connected(ClientIpStatus::Done(ip_settings))),
        ApStatus::Started(ApIpStatus::Done),
    ) = status
    {
        info!("Wifi connected");

        // ping(&ip_settings)?;
    } else {
        bail!("Unexpected Wifi status: {:?}", status);
    }

    Ok(wifi)
}

fn ping(ip_settings: &ipv4::ClientSettings) -> anyhow::Result<()> {
    info!("About to do some pings for {:?}", ip_settings);

    let ping_summary =
        ping::EspPing::default().ping(ip_settings.subnet.gateway, &Default::default())?;
    if ping_summary.transmitted != ping_summary.received {
        bail!(
            "Pinging gateway {} resulted in timeouts",
            ip_settings.subnet.gateway
        );
    }

    info!("Pinging done");

    Ok(())
}

fn mqtt() -> anyhow::Result<()> {
    let broker = ("m1", 1883);

    let mut options = MqttOptions::new("horse_client", broker.0, broker.1);
    options.set_credentials(MQTT_USER, MQTT_PASS);
    options.set_inflight(10);

    info!("connecting…");
    println!("prinz lem");
    let mut event_loop = EventLoop::new(options, 4);
    event_loop.requests_tx.send(Request::PingReq)?;
    event_loop.requests_tx.send(Request::Publish(Publish::new(
        "tropical",
        mqttbytes::QoS::AtLeastOnce,
        vec![1, 2, 3, 4],
    )))?;
    loop {
        match event_loop.poll()? {
            eventloop::Event::Incoming(packet) => {
                info!("[MQTT] {:?}", packet);
            }
            eventloop::Event::Outgoing(packet) => todo!(),
        }
    }
}
fn main() -> anyhow::Result<()> {
    // Temporary. Will disappear once ESP-IDF 4.4 is released, but for now it is necessary to call this function once,
    // or else some patches to the runtime implemented by esp-idf-sys might not link properly.
    esp_idf_sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    // Get backtraces from anyhow; only works for Xtensa arch currently
    // TODO: No longer working with ESP-IDF 4.3.1+
    //#[cfg(target_arch = "xtensa")]
    //env::set_var("RUST_BACKTRACE", "1");

    let peripherals = Peripherals::take().unwrap();
    let _pins = peripherals.pins;

    let netif_stack = Arc::new(EspNetifStack::new()?);
    let sys_loop_stack = Arc::new(EspSysLoopStack::new()?);
    let default_nvs = Arc::new(EspDefaultNvs::new()?);
    let mut _wifi = wifi(
        netif_stack.clone(),
        sys_loop_stack.clone(),
        default_nvs.clone(),
    )?;

    let res = mqtt();
    info!("con? {:?}", res);

    Ok(())
}
