mod proxy;

use crate::daemon::proxy::Proxy;
use crate::App;
use futures::executor::block_on;
use hotwatch::{Hotwatch};
use std::collections::HashMap;
use std::fs;
use std::fs::DirEntry;
use std::path::{PathBuf};
use std::str::FromStr;
use std::sync::mpsc::Receiver;
use std::sync::{mpsc, Arc};
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::sync::Mutex;
use tokio::time;
use log::{debug, error, info, warn};

// TODO: remove unnecessary unwraps (you know, do _actual_ error handling)

const FIFO: &str = "/var/tmp/dorc-fifo";
const APPS_DIR: &str = "/etc/dorc/apps/";

struct ProxiedApp {
    app: App,
    proxy: Arc<Mutex<Proxy>>,
}

impl ProxiedApp {
    fn from_app(app: App) -> ProxiedApp {
        let service_port = app.subservices.get(&app.active_service).unwrap().port;
        let proxy = Arc::new(Mutex::new(block_on(Proxy::new(app.listen_port, service_port)).unwrap()));


        Self { app, proxy }
    }
}

struct Daemon {
    receiver: Receiver<Commands>,
    apps: HashMap<PathBuf, ProxiedApp>,
    hotwatch: Hotwatch,
}

impl Daemon {
    fn new(rx: Receiver<Commands>) -> Self {
        let hotwatch = Hotwatch::new().expect("hotwatch failed to initialize!");

        let mut d = Daemon {
            receiver: rx,
            apps: HashMap::new(),
            hotwatch,
        };

        d
    }

    fn load_all_apps(&mut self) {
        // get all ok files in dir
        let app_files: Vec<DirEntry> = fs::read_dir(APPS_DIR)
            .unwrap()
            .filter_map(|p| p.ok())
            .filter(|p| p.path().is_file())
            .collect();

        self.apps = app_files
            .iter()
            .filter_map(|file| {
                match App::load(file.path()) {
                    Ok(app) => Some((file.path(), ProxiedApp::from_app(app))),
                    Err(e) => {
                        error!("Could not load file {} as app | {}", file.file_name().to_str().unwrap(), e);
                        None
                    },
                }
            })
            .collect();
    }

    fn recv_commands(&mut self) {
        if let Ok(command) = self.receiver.try_recv() {
            info!("Received {:?}", command);
            match command {
                Commands::Reload(arg) => {
                    let path = PathBuf::from_str(&format!("{}/{}.toml", APPS_DIR, arg)).unwrap();
                    // TODO: use a string instead of a path. That was a bad design decision based on being lazy.
                    // TODO: handle error when file isn't in app...
                    let app = &self.apps.get(&path).unwrap().app;

                    match &app.subservices.get(&app.active_service) {
                        None => {}
                        Some(service) => {
                            std::process::Command::new("systemctl")
                                .args(&["reload", &service.qualified_name])
                                .output()
                                .expect("failed to enable");

                            info!("'{}' has been reloaded.", service.qualified_name);
                        }
                    }
                }
            }
        }
    }

    async fn listen(&mut self) {
        self.recv_commands();

        for (_, app) in &mut self.apps {
            if !app.proxy.lock().await.is_listening {
                let tmp1 = app.proxy.clone();
                tokio::spawn(async move {
                    let tmp2 = tmp1.clone();
                    Proxy::listen(tmp2).await;
                });
            }
        }
    }

    async fn reroute_proxies(&mut self) {
        for (_, pa) in &mut self.apps {
            let (app, proxy) = (&mut pa.app, &mut pa.proxy.lock().await);
            proxy.reroute_to(app.subservices.get(&app.active_service).unwrap().port);
        }
    }
}

#[derive(Debug)]
pub enum Commands {
    Reload(String),
}

pub async fn start() {
    let (sender, receiver) = mpsc::channel();
    let mut daemon = Daemon::new(receiver);
    daemon.load_all_apps();
    // let's not hotwatch this dir - let's just call a command through the fifo fd
    // when applications are modified from dorc commands
    // TODO: watch app release-dir + bin, copy to inactive

    tokio::spawn(watch_fifo(sender.clone()));

    let mut interval = time::interval(time::Duration::from_millis(20));

    loop {
        interval.tick().await;
        daemon.listen().await;
    }
}

pub(crate) async fn watch_fifo(sender: mpsc::Sender<Commands>) {
    debug!("Watching FIFO command file...");
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
                        let arg = splitbuf[1];
                        sender
                            .send(Commands::Reload(arg.to_string().trim().to_string()))
                            .expect("failed to send reload");
                    } else {
                        // TODO: include instructions for using FIFO commands
                        error!("Too many arguments on reload command.");
                    }
                }
                _ => error!("{} is not a valid command.", command)
            }
            buf.clear();
        }
    }
}
