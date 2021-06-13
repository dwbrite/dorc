mod proxy;

use tokio::fs::File;
use tokio::net::TcpListener;
use tokio::time;
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;
use crate::proxy::Proxy;

const FIFO: &str = "/var/tmp/dorc-fifo";

struct _Service {
    qualified_name: String,
    workspace: String, // defaults to /srv/www/<qualified-service-name>
    port: u16,

    on_start: String,
    on_reload: Option<String>,
    on_restart: Option<String>, // defaults to stop; start
    on_stop: Option<String>, // defaults to kill <pid>
}

struct _App {
    listener: TcpListener,
    blue: _Service,
    green: _Service,
}

enum Commands {
    _Reload(String)
}

async fn watch_fifo(_sender: mpsc::Sender<Commands>) {
    let _ = unix_named_pipe::create(FIFO, None);

    let fd = File::open(FIFO).await.unwrap();
    let mut reader = tokio::io::BufReader::with_capacity(128, fd);
    let mut buf = String::new();

    //
    let mut interval = time::interval(time::Duration::from_secs(5));

    loop {
        interval.tick().await;

        let bytes_read = reader.read_line(&mut buf).await.unwrap();

        if bytes_read != 0 {
            // TODO: read buffer, match, send message
            print!("{}", buf);
            buf.clear();
        }
    }
}

#[tokio::main]
async fn main() {
    let (sender, _receiver) = mpsc::channel(16);

    let mut proxy = Proxy::new(41235, 41234).await.expect(":(");
    let a = proxy.listen();
    let b = watch_fifo(sender.clone());

    tokio::join!(a, b);
}
