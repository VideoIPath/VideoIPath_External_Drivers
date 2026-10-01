//! A bounded control-plane workload, not a synthetic claim about physical throughput.
#![cfg(feature = "device-lab")]

use rust_example::{
    application,
    lab::{client::Client, simulator},
    protocol::Request,
    registry::Registry,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn full_simulator_capacity_bulk_apply_and_repeated_cyclic_checks() {
    let temp = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let socket = temp.path().join("device.sock");
    let control = temp.path().join("control.sock");
    let (server_path, control_path) = (socket.clone(), control);
    let stop = CancellationToken::new();
    let server_stop = stop.clone();
    let server =
        tokio::spawn(
            async move { simulator::serve(&server_path, &control_path, server_stop).await },
        );
    let device = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(d) = Client::connect(&socket, Duration::from_secs(1)).await {
                break d;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let registry = Registry::new();
    let config = |id: &str, sub_index: u32| {
        json!({"id":id,"sub_index":sub_index,"transceiver_type":"sender","enabled_config":{
            "main_ip_destination":{"address":"239.1.1.1","udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}},"format_info":{}
        }})
    };
    let configs: Vec<Value> = ["tx-1", "tx-audio"]
        .into_iter()
        .flat_map(|id| (1..=64).map(move |n| config(id, n)))
        .collect();
    let request = |kind: &str, data: Value| Request {
        message_id: 1,
        message_type: kind.into(),
        data,
    };
    // Exercise the actual codec -> application -> Unix adapter -> simulator ->
    // readback path at the simulator's configured-resource ceiling.
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        application::handle(
            &device,
            &registry,
            request(
                "set_ipt_configurations",
                json!({"module_id":"module-1","sender_configs":configs}),
            ),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        result.response["data"],
        json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
    for _ in 0..10 {
        for config in &configs {
            let result = application::handle(
                &device,
                &registry,
                request(
                    "check_ipt_configuration",
                    json!({"module_id":"module-1","config":config,"set_again":false}),
                ),
            )
            .await;
            assert_eq!(result.response["data"], true);
        }
    }
    let result = application::handle(
        &device,
        &registry,
        request(
            "set_ipt_configurations",
            json!({"module_id":"module-1","sender_configs":[config("tx-asi",1)]}),
        ),
    )
    .await;
    assert_eq!(
        result.response["data"]["sender_config_errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // Saturation is reported per port; confirmed existing resources remain readable.
    let result = application::handle(
        &device,
        &registry,
        request(
            "check_ipt_configuration",
            json!({"module_id":"module-1","config":configs[0],"set_again":false}),
        ),
    )
    .await;
    assert_eq!(result.response["data"], true);
    device.close().await;
    stop.cancel();
    server.await.unwrap().unwrap();
}
