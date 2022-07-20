use anyhow::{bail, Result};
use as_any::Downcast;
use events::{Events, EventsState};
use events_api::event_handler::EventHandler;
use kv::{Kv, KvState};
use lockd_etcd::LockdEtcd;
use mq_azure_servicebus::MqAzureServiceBus;
use mq_filesystem::MqFilesystem;
use pubsub_confluent_kafka::PubSubConfluentKafka;
use http_api::HttpApi;
use runtime::{
    resource::{BasicState, StateTable},
    Builder,
};
use runtime_configs::{Configs, ConfigsState};
use std::sync::{Arc, Mutex};
use wit_bindgen_wasmtime::wasmtime::Store;
use runtime::resource::{Ctx, Resource};

use spiderlightning::core::slightfile::TomlFile;

const KV_HOST_IMPLEMENTORS: [&str; 2] = ["kv.filesystem", "kv.azblob"];
const CONFIGS_HOST_IMPLEMENTORS: [&str; 2] = ["configs.usersecrets", "configs.envvars"];

pub async fn handle_run(module: &str, toml: &TomlFile, toml_file_path: &str) -> Result<()> {
    tracing::info!("Starting slight");

    let resource_map = Arc::new(Mutex::new(StateTable::default()));

    let mut host_builder = Builder::new_default()?;
    let mut guest_builder = Builder::new_default()?;
    host_builder.link_wasi()?;
    guest_builder.link_wasi()?;
    let mut events_enabled = false;
    let mut http_enabled = false;
    if toml.specversion.as_ref().unwrap() == "0.1" {
        for c in toml.capability.as_ref().unwrap() {
            let resource_type: &str = c.name.as_str();
            match resource_type {
            "events" => {
                events_enabled = true;
                host_builder.link_capability::<Events>(resource_type.to_string(), EventsState::new(resource_map.clone()))?;
                guest_builder.link_capability::<Events>(resource_type.to_string(), EventsState::new(resource_map.clone()))?;
            },
            _ if KV_HOST_IMPLEMENTORS.contains(&resource_type) => {
                if let Some(ss) = &toml.secret_store {
                    host_builder.link_capability::<Kv>("kv".to_string(), KvState::new(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path)))?;
                    guest_builder.link_capability::<Kv>("kv".to_string(), KvState::new(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path)))?;
                } else {
                    bail!("the kv capability requires a secret store of some type (i.e., envvars, or usersecrets) specified in your config file so it knows where to grab, say, the AZURE_STORAGE_ACCOUNT, and AZURE_STORAGE_KEY from.")
                }
            }
            "mq.filesystem" => {
                host_builder.link_capability::<MqFilesystem>(resource_type.to_string(), resource_map.clone())?;
                guest_builder.link_capability::<MqFilesystem>(resource_type.to_string(), resource_map.clone())?;
            },
            "mq.azsbus" => {
                if let Some(ss) = &toml.secret_store {
                    host_builder.link_capability::<MqAzureServiceBus>(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path))?;
                    guest_builder.link_capability::<MqAzureServiceBus>(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path))?;
                } else {
                    bail!("the mq.azsbus capability requires a secret store of some type (i.e., envvars, or usersecrets) specified in your config file so it knows where to grab the AZURE_SERVICE_BUS_NAMESPACE, AZURE_POLICY_NAME, and AZURE_POLICY_KEY from.")
                }
            },
            "lockd.etcd" => {
                if let Some(ss) = &toml.secret_store {
                    host_builder.link_capability::<LockdEtcd>(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path))?;
                    guest_builder.link_capability::<LockdEtcd>(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path))?;
                } else {
                    bail!("the lockd.etcd capability requires a secret store of some type (i.e., envvars, or usersecrets) specified in your config file so it knows where to grab the ETCD_ENDPOINT.")
                }
            },
            "pubsub.confluent_kafka" => {
                if let Some(ss) = &toml.secret_store {
                    host_builder.link_capability::<PubSubConfluentKafka>(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path))?;
                    guest_builder.link_capability::<PubSubConfluentKafka>(resource_type.to_string(), BasicState::new(resource_map.clone(), ss, toml_file_path))?;
                } else {
                    bail!("the pubsub.confluent_kafka capability requires a secret store of some type (i.e., envvars, or usersecrets) specified in your config file so it knows where to grab the CK_SECURITY_PROTOCOL, CK_SASL_MECHANISMS, CK_SASL_USERNAME, CK_SASL_PASSWORD, and CK_GROUP_ID.")
                }
            },
            _ if CONFIGS_HOST_IMPLEMENTORS.contains(&resource_type) => {
                host_builder.link_capability::<Configs>(
                    "configs".to_string(),
                    ConfigsState::new(resource_map.clone(), resource_type, toml_file_path),
                )?;
                guest_builder.link_capability::<Configs>(
                    "configs".to_string(),
                    ConfigsState::new(resource_map.clone(), resource_type, toml_file_path),
                )?;
            },
            "http-api" => {
                http_enabled = true;
                host_builder.link_capability::<HttpApi>(resource_type.to_string(), resource_map.clone())?;
                guest_builder.link_capability::<HttpApi>(resource_type.to_string(), resource_map.clone())?;
            }
            _ => bail!("invalid url: currently slight only supports 'configs.usersecrets', 'configs.envvars', 'events', 'kv.filesystem', 'kv.azblob', 'mq.filesystem', 'mq.azsbus', 'lockd.etcd', and 'pubsub.confluent_kafka' schemes"),
        }
        }
    } else {
        bail!("unsupported toml spec version");
    }

    let (_, mut store, instance) = host_builder.build(module)?;
    let (_, mut store2, instance2) = guest_builder.build(module)?;
    if events_enabled {
        let event_handler = EventHandler::new(&mut store2, &instance2, |ctx| &mut ctx.state)?;
        let event_handler_resource: &mut Events = get_resource(&mut store, "events");
        event_handler_resource.update_state(
                Arc::new(Mutex::new(store2)),
                Arc::new(Mutex::new(event_handler)),
            )?;
    }
    tracing::info!("Executing {}", module);
    instance
        .get_typed_func::<(), _, _>(&mut store, "_start")?
        .call(&mut store, ())?;

    if http_enabled {
        shutdown_signal().await;
        let http_api_resource: &mut HttpApi = get_resource(&mut store, "http-api");
        let _ = http_api_resource.close();
    }
    Ok(())
}

fn get_resource<'a, T>(store: &'a mut Store<Ctx>, scheme_name: &'a str) -> &'a mut T where T: Resource {
    store
        .data_mut()
        .data
        .get_mut(scheme_name)
        .expect("internal error: resource_map does not contain key events")
        .0
        .as_mut()
        .downcast_mut::<T>()
        .expect("internal error: resource map contains key events but can't downcast")
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}
