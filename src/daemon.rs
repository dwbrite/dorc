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

// TODO: remove unnecessary unwraps (you know, do _actual_ error handling)

const FIFO: &str = "/var/tmp/dorc-fifo";
const APPS_DIR: &str = "/etc/dorc/apps/";

struct ProxiedApp {
    app: App,
    proxy: Proxy,
}

impl ProxiedApp {
    fn from_app(app: App) -> ProxiedApp {
        let service_port = app.subservices.get(&app.active_service).unwrap().port;
        let proxy = block_on(Proxy::new(app.listen_port, service_port)).unwrap();

        Self { app, proxy }
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
            match command {
                Commands::Reload(arg) => {
                    let path = PathBuf::from_str(&format!("{}/{}.toml", APPS_DIR, arg)).unwrap();
                    let app = &self.apps.get(&path).unwrap().app;
                    match &app.subservices.get(&app.app_name) {
                        None => {}
                        Some(service) => {
                            // :shrugs:
                            std::process::Command::new("systemctl")
                                .args(&["reload", &service.qualified_name])
                                .output()
                                .expect("failed to enable");
                        }
                    }
                }
            }
        }
    }

    async fn listen(&mut self) {
        loop {
            self.recv_commands();

            let apps = &mut self.apps;

            for (_, app) in apps {
                if !app.proxy.is_listening {
                    app.proxy.listen().await;
                }
            }
        }
    }

    fn reroute_proxies(&mut self) {
        for (_, pa) in &mut self.apps {
            let (app, proxy) = (&mut pa.app, &mut pa.proxy);
            proxy.reroute_to(app.subservices.get(&app.active_service).unwrap().port);
        }
    }
}

struct _Service {
    qualified_name: String,
    workspace: String, // defaults to /srv/www/<qualified-service-name>
    port: u16,

    on_start: String,
    on_reload: Option<String>,
    on_restart: Option<String>, // defaults to stop; start
    on_stop: Option<String>,    // defaults to kill <pid>
}

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
            let mut daemon = block_on(d1.lock());
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
                    }
                }
                DebouncedEvent::Rename(a, b) => {
                    let app = App::load(&b);
                    if app.is_ok() {
                        daemon
                            .apps
                            .insert(b.clone(), ProxiedApp::from_app(app.unwrap()));
                    }
                    daemon.apps.remove(&a);
                }
                _ => {}
            }
            daemon.reroute_proxies();
        })
        .expect("failed to watch file!");

    // TODO: watch app release-dir + bin, copy to inactive
    // TODO: make sure all apps are running

    let mut lock = daemon.lock().await;
    let a = lock.listen();
    let b = watch_fifo(sender.clone());

    tokio::join!(a, b);
}

pub(crate) async fn watch_fifo(sender: mpsc::Sender<Commands>) {
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
                            .send(Commands::Reload(arg.to_string()))
                            .expect("failed to send reload");
                    } else {
                        // TODO: log error
                    }
                }
                _ => { /* log error */ }
            }
            buf.clear();
        }
    }
}
