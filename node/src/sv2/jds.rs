use jd_server::lib::*;

pub async fn jd_server() {
    // Load config
    let config_path = "./config/jd-server-config.toml";

    let config: jd_server::Configuration = match std::fs::read_to_string(config_path) {
        Ok(c) => match toml::from_str(&c) {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to parse config:{:?}", e);
                return;
            }
        },
        Err(e) => {
            log::error!("Failed to read config: {}", e);
            return;
        }
    };

    let url = config.core_rpc_url.clone() + ":" + &config.core_rpc_port.clone().to_string();
    let username = config.core_rpc_user.clone();
    let password = config.core_rpc_pass.clone();
    let mempool = std::sync::Arc::new(tokio::sync::Mutex::new(jd_server::lib::mempool::JDsMempool::new(
        url.clone(),
        username,
        password,
    )));
    let mempool_cloned_ = mempool.clone();
    if url.contains("http") {
        tokio::task::spawn(async move {
            loop {
                let _ = jd_server::mempool::JDsMempool::update_mempool(mempool_cloned_.clone()).await;
                // TODO this should be configurable by the user
                tokio::time::sleep(tokio::time::Duration::from_millis(10000)).await;
            }
        });
    };

    let (status_tx, status_rx) = async_channel::unbounded();
    log::info!("Jds INITIALIZING with config: {:?}", config_path);

    let cloned = config.clone();
    let sender = jd_server::status::Sender::Downstream(status_tx.clone());
    let mempool_cloned = mempool.clone();
    tokio::task::spawn(async move { jd_server::lib::job_declarator::JobDeclarator::start(cloned, sender, mempool_cloned).await });

    // Start the error handling loop
    // See `./status.rs` and `utils/error_handling` for information on how this operates
    loop {
        let task_status = tokio::select! {
            task_status = status_rx.recv() => task_status,
            interrupt_signal = tokio::signal::ctrl_c() => {
                match interrupt_signal {
                    Ok(()) => {
                        log::info!("Interrupt received");
                    },
                    Err(err) => {
                        log::error!("Unable to listen for interrupt signal: {}", err);
                        log::error!("Halting.");
                        std::process::exit(1);
                    },
                }
                break;
            }
        };
        let task_status: jd_server::status::Status = task_status.unwrap();

        match task_status.state {
            // Should only be sent by the downstream listener
            jd_server::status::State::DownstreamShutdown(err) => {
                log::error!(
                    "SHUTDOWN from Downstream: {}\nTry to restart the downstream listener",
                    err
                );
            }
            jd_server::status::State::TemplateProviderShutdown(err) => {
                log::error!("SHUTDOWN from Upstream: {}\nTry to reconnecting or connecting to a new upstream", err);
                break;
            }
            jd_server::status::State::Healthy(msg) => {
                log::info!("HEALTHY message: {}", msg);
            }
            jd_server::status::State::DownstreamInstanceDropped(downstream_id) => {
                log::warn!("Dropping downstream instance {} from jds", downstream_id);
            }
        }
    }
}
