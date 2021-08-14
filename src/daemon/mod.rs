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
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{mpsc, Arc};
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::sync::Mutex;
use tokio::time;
use log::*;
use hotwatch::notify::DebouncedEvent;

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
    apps: HashMap<PathBuf, ProxiedApp>,
    hotwatch: Hotwatch,
    cmd_tx: Sender<Commands>,
    cmd_rx: Receiver<Commands>
}

impl Daemon {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();

        let hotwatch = Hotwatch::new().expect("hotwatch failed to initialize!");

        Daemon {
            apps: HashMap::new(),
            hotwatch,
            cmd_tx: sender,
            cmd_rx: receiver
        }
    }

    fn load_all_apps(&mut self) {
        // get all ok files in dir
        let app_files: Vec<DirEntry> = fs::read_dir(APPS_DIR)
            .unwrap()
            .filter_map(|p| p.ok())
            .filter(|p| p.path().is_file())
            .collect();

        for app in app_files {
            self.load_app(app.path());
        }
    }

    fn hotwatch_release(&mut self, path: PathBuf) {
        let sender = self.cmd_tx.clone();
        let result = self.hotwatch.watch(path.clone(), move |event| {
            match event {
                DebouncedEvent::Error(e, p) => error!("error while watching {:?}: {}", p, e),
                _ => sender.send(Commands::CopyRelease(path.clone())).unwrap()
            }
        });

        if result.is_err() {
            error!("failed to hotwatch: {}", result.err().unwrap());
        }
    }

    fn recv_commands(&mut self) {
        // TODO: should try_recv() be in a loop? or could that cause unwanted latency?
        if let Ok(command) = self.cmd_rx.try_recv() {
            info!("Received {:?}", command);
            match command {
                Commands::Reload(arg) => {
                    let path = PathBuf::from_str(&format!("{}/{}.toml", APPS_DIR, arg)).unwrap();
                    self.reload_app(path);
                }
                Commands::Load(path) => self.load_app(PathBuf::from(path)),
                Commands::CopyRelease(path) => self.copy_release(PathBuf::from(path))
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

    fn load_app(&mut self, path: PathBuf) {
        match App::load(&path) {
            Ok(app) => {
                self.apps.insert(path.clone(), ProxiedApp::from_app(app)); // ignore old value
                self.hotwatch_release(path.clone())
            },
            Err(e) => { error!("Could not load file {:?} as app | {}", path.file_name(), e); },
        }
    }

    fn reload_app(&mut self, path: PathBuf) {
        let opt_app = self.apps.get(&path);
        if let None = opt_app {
            error!("Failed to reload app from path: {:?}", path);
            return;
        }

        let app = &opt_app.unwrap().app;
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

    fn copy_release(&mut self, path: PathBuf) {
        let proxied_app = self.apps.get(&path).expect("Failed to copy release from an unloaded application");
        let app = &proxied_app.app;
        let active = app.active_service.clone();

        // TODO: in the future I may want to have a smart pointer to the active an inactive service instead of using a hashmap+string
        // copy files from inactive service
        for (key, service) in &app.subservices {
            if key.ne(&active) {
                app.migrate_service(service);
            }
        }
    }
}

#[derive(Debug)]
pub enum Commands {
    Reload(String),
    Load(String),
    CopyRelease(PathBuf),
}

pub async fn start() {
    let mut daemon = Daemon::new();
    daemon.load_all_apps();
    // let's not hotwatch this dir - let's just call a command through the fifo fd
    // when applications are modified from dorc commands
    // TODO: watch app release-dir + bin, copy to inactive

    tokio::spawn(watch_fifo(daemon.cmd_tx.clone()));

    let mut interval = time::interval(time::Duration::from_millis(20));

    loop {
        interval.tick().await;
        daemon.listen().await;
    }
}

pub(crate) async fn watch_fifo(sender: mpsc::Sender<Commands>) {
    debug!("Watching FIFO command file...");
    let _ = unix_named_pipe::create(FIFO, None);

    let fd = File::open(FIFO).await.expect(&format!("Failed to open FIFO file descriptor {}", FIFO));

    let mut reader = tokio::io::BufReader::with_capacity(128, fd);
    let mut buf = String::new();

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
