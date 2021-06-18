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

pub enum Commands {
    _Reload(String)
}

pub async fn start() {
    // TODO: watch the app files
    // TODO: watch app release-dir + bin, copy to inactive

    let (sender, _receiver) = mpsc::channel(16);

    let app = crate::App::load("dwbrite.com".to_string()).expect("couldn't open app file");

    let mut proxy = Proxy::new(app.listen_port, app.subservices.get(&app.active_service).unwrap().port).await.expect(":(");
    let a = proxy.listen();
    let b = watch_fifo(sender.clone());

    tokio::join!(a, b);
}

pub(crate) async fn watch_fifo(_sender: mpsc::Sender<Commands>) {
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
            let splitbuf: Vec<&str> = buf.splitn(2, " ").collect();

            let command = {
                if splitbuf.len() >= 1 {
                    splitbuf[0]
                } else {
                    buf.clear();
                    continue;
                }
            };



            match command {
                "reload" => {
                    if splitbuf.len() == 2 {
                        // call function with argument
                        let arg = splitbuf[1];
                    } else {
                        // TODO: log error
                    }
                }
                _ => { /* log error */}
            }
            buf.clear();
        }
    }
}