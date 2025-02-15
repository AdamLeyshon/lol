use anyhow::ensure;
use anyhow::Result;
use bollard::*;
use log::*;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::transport::{Channel, Endpoint, Uri};

const NETWORK_NAME: &str = "lol_raft-network";

#[derive(Clone)]
struct Container(String);

pub struct Env {
    docker: Arc<Docker>,
    containers: HashMap<u8, Container>,
}
impl Env {
    pub fn new() -> Result<Self> {
        let docker = Docker::connect_with_socket_defaults()?;
        Ok(Self {
            docker: docker.into(),
            containers: HashMap::new(),
        })
    }

    pub fn id_from_address(address: &str) -> u8 {
        let id = address
            .strip_prefix("http://lol-testapp-")
            .unwrap()
            .strip_suffix(":50000")
            .unwrap();
        id.parse().unwrap()
    }

    pub fn address_from_id(id: u8) -> String {
        format!("http://lol-testapp-{id}:50000")
    }

    pub async fn create(&mut self, id: u8) -> Result<()> {
        ensure!(!self.containers.contains_key(&id));
        let options = container::CreateContainerOptions {
            name: format!("lol-testapp-{}", id),
            ..Default::default()
        };
        let config = container::Config {
            image: Some("lol-testapp:latest".to_string()),
            env: Some(vec![
                format!("address=http://lol-testapp-{id}:50000"),
                "RUST_LOG=info".to_string(),
            ]),
            ..Default::default()
        };
        let resp = self.docker.create_container(Some(options), config).await?;
        let container_id = resp.id;
        self.containers.insert(id, Container(container_id));
        Ok(())
    }

    pub async fn start(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = &self.containers.get(&id).unwrap().0.clone();
        self.docker
            .start_container::<&str>(&container_id, None)
            .await?;
        Ok(())
    }

    pub async fn stop(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = self.containers.get(&id).unwrap().0.clone();
        self.docker.stop_container(&container_id, None).await?;
        Ok(())
    }

    pub async fn pause(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = self.containers.get(&id).unwrap().0.clone();
        self.docker.pause_container(&container_id).await?;
        Ok(())
    }

    pub async fn resume(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = self.containers.get(&id).unwrap().0.clone();
        self.docker.unpause_container(&container_id).await?;
        Ok(())
    }

    pub async fn pause_v2(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = self.containers.get(&id).unwrap().0.clone();

        let inspect = self.docker.inspect_container(&container_id, None).await?;
        let pid = inspect
            .state
            .and_then(|state| state.pid)
            .ok_or(anyhow::anyhow!("no pid"))?;

        let mut child = tokio::process::Command::new("kill")
            .arg("-s")
            .arg("SIGSTOP")
            .arg(format!("{pid}"))
            .spawn()?;
        child.wait().await?;

        Ok(())
    }

    pub async fn resume_v2(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = self.containers.get(&id).unwrap().0.clone();

        let inspect = self.docker.inspect_container(&container_id, None).await?;
        let pid = inspect
            .state
            .and_then(|state| state.pid)
            .ok_or(anyhow::anyhow!("no pid"))?;

        let mut child = tokio::process::Command::new("kill")
            .arg("-s")
            .arg("SIGCONT")
            .arg(format!("{pid}"))
            .spawn()?;
        child.wait().await?;

        Ok(())
    }

    pub async fn connect_network(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = self.containers.get(&id).unwrap().0.clone();
        let config = network::ConnectNetworkOptions {
            container: container_id,
            ..Default::default()
        };
        self.docker.connect_network(NETWORK_NAME, config).await?;

        let config = network::InspectNetworkOptions {
            verbose: true,
            ..Default::default()
        };
        dbg!(
            self.docker
                .inspect_network::<&str>(NETWORK_NAME, Some(config))
                .await?
        );

        Ok(())
    }

    pub async fn disconnect_network(&mut self, id: u8) -> Result<()> {
        ensure!(self.containers.contains_key(&id));
        let container_id = self.containers.get(&id).unwrap().0.clone();
        let config = network::DisconnectNetworkOptions {
            container: container_id,
            ..Default::default()
        };
        self.docker.disconnect_network(NETWORK_NAME, config).await?;
        Ok(())
    }

    pub async fn ping(&self, id: u8) -> Result<()> {
        let chan = self.connect(0);
        let mut cli = testapp::PingClient::new(chan);
        cli.ping(()).await?;
        Ok(())
    }

    pub fn connect(&self, id: u8) -> Channel {
        let uri: Uri = Self::address_from_id(id).parse().unwrap();
        let endpoint = Endpoint::from(uri)
            .timeout(std::time::Duration::from_secs(1))
            .connect_timeout(std::time::Duration::from_secs(1));
        let chan = endpoint.connect_lazy();
        chan
    }
}
impl Drop for Env {
    fn drop(&mut self) {
        for (id, container) in self.containers.drain() {
            let docker = self.docker.clone();
            let fut = async move {
                let resp = docker
                    .remove_container(
                        &container.0,
                        Some(container::RemoveContainerOptions {
                            force: true,
                            v: true,
                            ..Default::default()
                        }),
                    )
                    .await;
                match resp {
                    Ok(_) => info!("removed container {id}"),
                    Err(e) => error!("failed to remove container {id} (err={e})"),
                }
            };
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(fut);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn id_address() {
        for id in 0..=255 {
            let address = Env::address_from_id(id);
            assert_eq!(id, Env::id_from_address(&address));
        }
    }
}
