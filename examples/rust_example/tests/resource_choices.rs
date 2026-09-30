//! Resource links describe allowed local choices, not VideoIPath connections.
//! Observe the selected partner through the independently owned target state.
use rust_example::{
    application,
    device::Client,
    domain::{Device, Direction, PortKey},
    protocol::Request,
    registry::{Registry, validate_structure},
    simulator,
};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, time::Duration};
use tokio_util::sync::CancellationToken;

#[test]
fn resource_links_preserve_scoped_ids_and_ignore_duplicate_links() {
    let mut structure = Registry::new().structure();
    let first = structure["resource_links"][0].clone();
    structure["resource_links"]
        .as_array_mut()
        .unwrap()
        .push(first);
    validate_structure(&structure).unwrap();
    // The same local ID in the sender and receiver lists is legal. Direction
    // comes from the documented link kind; never infer it from an ID's spelling.
    let mut sender = structure["modules"][0]["senders"][0].clone();
    sender["id"] = json!("rx-1");
    structure["modules"][0]["senders"]
        .as_array_mut()
        .unwrap()
        .push(sender);
    structure["resource_links"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "type":"tx-tx","from":{"module_id":"module-1","port_id":"rx-1"},
            "to":{"module_id":"module-1","port_id":"tx-1"}
        }));
    validate_structure(&structure).unwrap();
    // This proves structural validation only, NOT tx-tx runtime partner semantics.
    let last = structure["resource_links"].as_array().unwrap().len() - 1;
    structure["resource_links"][last]["from"]["port_id"] = json!("rx-alt");
    assert!(validate_structure(&structure).is_err());
}

#[tokio::test]
async fn receiver_choices_are_applied_read_back_checked_and_removed_independently() {
    let dir = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let socket = dir.path().join("target.sock");
    let control = dir.path().join("control.sock");
    let stop = CancellationToken::new();
    let (path, token) = (socket.clone(), stop.clone());
    let server = tokio::spawn(async move { simulator::serve(&path, &control, token).await });
    let device = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(client) = Client::connect(&socket, Duration::from_secs(1)).await {
                break client;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let registry = Registry::new();
    registry.validate().unwrap();
    let receivers:Vec<_>=["rx-1","rx-alt"].into_iter().map(|id|json!({
        "id":id,"sub_index":1,"transceiver_type":"receiver","enabled_config":{
            "main_ip_source":{"source_address":"192.0.2.20","destination_address":"239.1.1.1","udp_port":5004,
            "network_interface":{"module_id":"module-1","port_id":"net-main"}}
        }
    })).collect();
    let request = |kind: &str, data: Value| Request {
        message_id: 1,
        message_type: kind.into(),
        data,
    };
    let configured = application::handle(
        &device,
        &registry,
        request(
            "set_ipt_configurations",
            json!({"module_id":"module-1","receiver_configs":receivers}),
        ),
    )
    .await;
    assert_eq!(
        configured.response["data"],
        json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
    let sender_key = PortKey {
        module_id: "module-1".into(),
        port_id: "tx-1".into(),
        sub_index: Some(1),
        direction: Direction::Sender,
    };
    for selected in ["rx-1", "rx-alt"] {
        let config = json!({"id":"tx-1","sub_index":1,"transceiver_type":"sender","enabled_config":{
            "main_ip_destination":{"address":"239.1.1.2","udp_port":5006,"network_interface":{"module_id":"module-1","port_id":"net-main"}},
            "resource_partner":{"module_id":"module-1","port_id":selected,"sub_index":1}
        }});
        let result = application::handle(
            &device,
            &registry,
            request(
                "set_ipt_configurations",
                json!({"module_id":"module-1","sender_configs":[config]}),
            ),
        )
        .await;
        assert_eq!(result.response["data"]["sender_config_errors"], json!([]));
        let observed = device
            .configuration(sender_key.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.partner.as_ref().unwrap().port_id, selected);
        assert_eq!(observed.partner.as_ref().unwrap().sub_index, Some(1));
        let check = application::handle(
            &device,
            &registry,
            request(
                "check_ipt_configuration",
                json!({"module_id":"module-1","config":config,"set_again":false}),
            ),
        )
        .await;
        assert_eq!(check.response["data"], true);
        let mut illegal = config.clone();
        illegal["enabled_config"]["resource_partner"]["port_id"] = json!("rx-audio");
        let rejected = application::handle(
            &device,
            &registry,
            request(
                "set_ipt_configurations",
                json!({"module_id":"module-1","sender_configs":[illegal]}),
            ),
        )
        .await;
        assert_eq!(
            rejected.response["data"]["sender_config_errors"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            device.configuration(sender_key.clone()).await.unwrap(),
            Some(observed)
        );
    }
    let disabled=application::handle(&device,&registry,request("set_ipt_configurations",json!({"module_id":"module-1","sender_configs":[{"id":"tx-1","sub_index":1,"transceiver_type":"sender","enabled_config":null}]}))).await;
    assert_eq!(disabled.response["data"]["sender_config_errors"], json!([]));
    assert!(device.configuration(sender_key).await.unwrap().is_none());
    for id in ["rx-1", "rx-alt"] {
        let key = PortKey {
            module_id: "module-1".into(),
            port_id: id.into(),
            sub_index: Some(1),
            direction: Direction::Receiver,
        };
        assert!(device.configuration(key).await.unwrap().is_some());
    }
    device.close().await;
    stop.cancel();
    server.await.unwrap().unwrap();
}
