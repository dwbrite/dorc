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
use anyhow::*;

// TODO: remove unnecessary unwraps (you know, do _actual_ error handling)

pub(crate) const FIFO: &str = "/var/tmp/dorc-fifo";
const APPS_DIR: &str = "/etc/dorc/apps/";

struct ProxiedApp {
    app: App,
    proxy: Arc<Mutex<Proxy>>,
}

impl ProxiedApp {
    fn from_app(app: App) -> Result<ProxiedApp> {
        let service_port = app.active_service.port;
        let res_proxy = block_on(Proxy::new(app.listen_port, service_port))?;
        let proxy = Arc::new(Mutex::new(res_proxy));

        Ok(Self { app, proxy })
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
                Commands::Reload(name) => self.reload_app(app_pathbuf(name)),
                Commands::Load(name) => self.load_app(app_pathbuf(name)),
                Commands::Switch(name) => block_on(self.switch_active(app_pathbuf(name))),
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

    fn load_app(&mut self, path: PathBuf) {
        info!("Loading app: {}", path.to_str().unwrap());

        let res_app = App::load(&path);

        if let Err(e) = res_app {
            error!("Could not load file {:?} as app | {}", path.file_name(), e);
            return
        }

        let res_proxied_app = ProxiedApp::from_app(res_app.unwrap());

        if let Err(e) = res_proxied_app {
            error!("Could not create ProxiedApp: {}", e);
            return
        }

        self.apps.insert(path.clone(), res_proxied_app.unwrap()); // ignore old value
        self.hotwatch_release(path.clone());
    }

    fn reload_app(&mut self, path: PathBuf) {
        let opt_app = self.apps.get(&path);
        if let None = opt_app {
            error!("Failed to reload app from path: {:?}", path);
            return;
        }

        let app = &opt_app.unwrap().app;
        std::process::Command::new("systemctl")
            .args(&["reload", &app.active_service.qualified_name])
            .output()
            .expect("failed to enable");

        info!("'{}' has been reloaded.", app.active_service.qualified_name);

    }

    fn copy_release(&mut self, path: PathBuf) {
        let proxied_app = self.apps.get(&path).expect("Failed to copy release from an unloaded application");
        let app = &proxied_app.app;

        // copy files from inactive service
        if let Err(e) = app.migrate_service(&app.inactive_service) {
            error!("Failed to copy release directory for {}: {}", app.app_name, e);
        }
    }
    async fn switch_active(&mut self, path: PathBuf) {
        if let Some(proxied_app) = self.apps.get_mut(&path) {
            let (app, proxy) = (&mut proxied_app.app, &mut proxied_app.proxy.lock().await);
            app.swap_active();
            proxy.reroute_to(app.active_service.port);
            app.save();
        } else {
            error!("Could not retrieve app from {}", path.to_str().unwrap())
        }
    }
}

#[derive(Debug)]
pub enum Commands {
    Reload(String),
    Load(String),
    Switch(String),
    CopyRelease(PathBuf),
}

pub async fn start() {
    let mut daemon = Daemon::new();
    daemon.load_all_apps();

    tokio::spawn(watch_fifo(daemon.cmd_tx.clone()));

    let mut interval = time::interval(time::Duration::from_millis(20));

    loop {
        interval.tick().await;
        daemon.listen().await;
    }
}

fn app_pathbuf(app_name: String) -> PathBuf {
    PathBuf::from_str(&format!("{}{}.toml", APPS_DIR, app_name)).unwrap()
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
                        sender.send(Commands::Reload(splitbuf[1].trim().to_string()))
                            .expect("failed to send reload");
                    } else {
                        error!("Wrong number of arguments on reload command.");
                    }
                }
                "load" => {
                    if splitbuf.len() == 2 {
                        sender.send(Commands::Load(splitbuf[1].trim().to_string()))
                            .expect("failed to send load");
                    } else {
                        error!("Wrong number of arguments on load command.");
                    }
                }
                "switch" => {
                    if splitbuf.len() == 2 {
                        sender.send(Commands::Switch(splitbuf[1].trim().to_string()))
                            .expect("failed to send switch");
                    } else {
                        error!("Wrong number of arguments on switch command.");
                    }
                }
                _ => error!("{} is not a valid command.", command)
            }
            buf.clear();
        }
    }
}
