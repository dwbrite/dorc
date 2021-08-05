use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use futures::FutureExt;
use std::error::Error;

pub(crate) struct Proxy {
    listener: TcpListener,
    pub(crate) route: String,
}

// largely taken from tokio's proxy example
impl Proxy {
    pub async fn new(listener_port: u16, server_port: u16) -> Result<Proxy, Box<dyn Error>> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", listener_port)).await?;
        Ok(Proxy {
            listener,
            route: format!("127.0.0.1:{}", server_port),
        })
    }

    pub fn reroute_to(&mut self, server_port: u16) {
        self.route = format!("127.0.0.1:{}", server_port);
    }

    pub async fn listen(&mut self) {
        while let Ok((inbound, _)) = self.listener.accept().await {
            let transfer = transfer(inbound, self.route.clone()).map(|r| {
                if let Err(e) = r {
                    // TODO: log errors
                    println!("Failed to transfer; error={}", e);
                }
            });
            tokio::spawn(transfer);
        }
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
