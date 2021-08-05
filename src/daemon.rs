use crate::proxy::Proxy;
use crate::App;
use futures::executor::block_on;
use hotwatch::notify::DebouncedEvent;
use hotwatch::{Event, Hotwatch};
use std::collections::HashMap;
use std::fs;
use std::fs::DirEntry;
use std::path::PathBuf;
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
    is_listening: bool,
}

impl ProxiedApp {
    fn from_app(app: App) -> ProxiedApp {
        let service_port = app.subservices.get(&app.active_service).unwrap().port;
        let proxy = Arc::new(Mutex::new(block_on(Proxy::new(app.listen_port, service_port)).unwrap()));

        Self { app, proxy, is_listening: false }
    }
}

struct Daemon {
    receiver: Receiver<Commands>,
    apps: HashMap<PathBuf, ProxiedApp>,
}

impl Daemon {
    fn new(rx: Receiver<Commands>) -> Self {
        let mut d = Daemon {
            receiver: rx,
            apps: HashMap::new(),
        };

        // get all ok files in dir
        let app_files: Vec<DirEntry> = fs::read_dir(APPS_DIR)
            .unwrap()
            .filter_map(|p| p.ok())
            .filter(|p| p.path().is_file())
            .collect();

        d.apps = app_files
            .iter()
            .filter_map(|file| match App::load(file.path()) {
                Ok(app) => Some((file.path(), ProxiedApp::from_app(app))),
                Err(_) => None,
            })
            .collect();

        d
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

        let apps = &mut self.apps;

        for (_, app) in apps {
            let proxy = app.proxy.clone();
            if !app.is_listening {
                let p1 = proxy.clone();
                tokio::spawn(async move {
                    let mut lock = p1.lock().await;
                    lock.listen().await;
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
    let daemon = Arc::new(Mutex::new(Daemon::new(receiver)));
    let d1 = daemon.clone();
    let mut hotwatch = Hotwatch::new().expect("hotwatch failed to initialize!");
    hotwatch
        .watch("/etc/dorc/apps/", move |event: Event| {
            debug!("Hotwatch found changed files. Handling event: {:?}", &event);
            debug!("Locking daemon guard...");
            let mut daemon = block_on(d1.lock());
            debug!("Daemon locked.");
            match event {
                DebouncedEvent::Remove(p) => {
                    daemon.apps.remove(&p);
                }
                DebouncedEvent::Create(p) | DebouncedEvent::Write(p) => {
                    let app = App::load(&p);
                    if app.is_ok() {
                        daemon
                            .apps
                            .insert(p.clone(), ProxiedApp::from_app(app.unwrap()));
                    } else {
                        warn!("Application deserialization failed on '{}' -- Only dorc's .toml files should be placed here.", p.to_str().unwrap())
                    }
                }
                DebouncedEvent::Rename(a, b) => {
                    let app = App::load(&b);
                    // for now, the app needs to be removed first so that there's no port conflicts
                    // TODO: intelligently reroute proxies if the listen ports are identical
                    daemon.apps.remove(&a);
                    if app.is_ok() {
                        daemon
                            .apps
                            .insert(b.clone(), ProxiedApp::from_app(app.unwrap()));
                    } else {
                        warn!("Application deserialization failed on '{}' -- Only dorc's .toml files should be placed here.", b.to_str().unwrap())
                    }
                    info!("'{}' renamed to '{}'. There may be a brief service interruption.", a.to_str().unwrap(), b.to_str().unwrap());
                }
                _ => {}
            }
            block_on(daemon.reroute_proxies());
        })
        .expect("failed to watch file!");

    // TODO: watch app release-dir + bin, copy to inactive
    // TODO: make sure all apps are running

    tokio::spawn(watch_fifo(sender.clone()));

    let mut interval = time::interval(time::Duration::from_secs(2));

    loop {
        interval.tick().await;
        let mut d = daemon.lock().await;
        d.listen().await;
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
