use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use futures::FutureExt;
use std::error::Error;
use log::{debug, trace, info, error, warn};
use tokio::sync::Mutex;
use std::sync::Arc;
use std::time::Duration;

pub(crate) struct Proxy {
    pub(crate) listener: TcpListener,
    pub(crate) route: String,
    pub(crate) is_listening: bool,
}

// largely taken from tokio's proxy example
impl Proxy {
    pub async fn new(listener_port: u16, server_port: u16) -> Result<Proxy, Box<dyn Error>> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", listener_port)).await?;
        Ok(Proxy {
            listener,
            route: format!("127.0.0.1:{}", server_port),
            is_listening: false
        })
    }

    pub fn reroute_to(&mut self, server_port: u16) {
        self.route = format!("127.0.0.1:{}", server_port);
    }

    pub async fn listen(s: Arc<Mutex<Proxy>>) {
        Proxy::set_is_listening(s.clone(), true).await;
        // Lock proxy for up to 500ms at a time, allowing it to be modified between loops
        let mut ok = true;
        while ok {
            let guard = s.lock().await;
            if let Ok(result) = tokio::time::timeout(Duration::from_millis(500), guard.listener.accept()).await {
                ok = result.is_ok();

                if let Ok((inbound, _)) = result {
                    let transfer = transfer(inbound, guard.route.clone()).map(|r| {
                        if let Err(e) = r {
                            error!("Failed to transfer; error={}", e);
                        }
                    });
                    tokio::spawn(transfer);
                }
            }
        }
        Proxy::set_is_listening(s.clone(), false).await;
    }

    pub async fn set_is_listening(s: Arc<Mutex<Proxy>>, b: bool) {
        let mut guard = s.lock().await;
        guard.is_listening = b;
    }
}

async fn transfer(mut inbound: TcpStream, proxy_addr: String) -> Result<(), Box<dyn Error>> {
    let mut outbound = TcpStream::connect(proxy_addr).await?;

    let (mut ri, mut wi) = inbound.split();
    let (mut ro, mut wo) = outbound.split();

    let client_to_server = async {
        io::copy(&mut ri, &mut wo).await?;
        wo.shutdown().await
    };

    let server_to_client = async {
        io::copy(&mut ro, &mut wi).await?;
        wi.shutdown().await
    };

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}
