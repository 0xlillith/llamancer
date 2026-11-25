//! Foreground router supervisor for systemd or manual use; no service is installed automatically.
use crate::{config::Config, llama::session::Controller, models};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
pub fn run(config: Config, stop: Arc<AtomicBool>) -> io::Result<()> {
    let scan = models::scan(config.scan_roots(), &stop)?;
    let groups = config.filter_groups(models::group::group(scan.candidates));
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut controller = Controller::default();
    controller.start(&config, groups, None)?;
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut ready = false;
    loop {
        if stop.load(Ordering::Acquire) {
            controller.shutdown()?;
            return Ok(());
        }
        controller.tick(&config);
        if controller.snapshot.as_ref().is_some_and(|s| s.router) && !ready {
            ready = true;
            eprintln!("Llamancer router ready at {}", config.endpoint);
        }
        if !controller.active() {
            return Err(io::Error::other(format!(
                "Router stopped: {}",
                controller.status
            )));
        }
        if !ready && Instant::now() >= deadline {
            controller.shutdown()?;
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Router startup timeout",
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
pub fn from_config() -> io::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&stop))?;
    signal_hook::flag::register(signal_hook::consts::SIGHUP, Arc::clone(&stop))?;
    let config = Config::load(&crate::config::config_dir()?.join("config.toml"))?;
    run(config, stop)
}
