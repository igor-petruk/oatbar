#![allow(dead_code)]

use std::{
    sync::{Arc, Mutex},
    thread::sleep,
    time::SystemTime,
};

use crate::thread;

#[derive(Clone, Debug)]
pub struct Timer {
    at: Arc<Mutex<SystemTime>>,
}

impl Timer {
    pub fn new<F>(name: &str, at: SystemTime, f: F) -> anyhow::Result<Self>
    where
        F: Fn() + Send + 'static,
    {
        let timer = Timer {
            at: Arc::new(Mutex::new(at)),
        };
        {
            let timer = timer.clone();
            thread::spawn_loop(name, move || {
                let at = timer.elapses_at();
                match at.duration_since(SystemTime::now()) {
                    Ok(duration) => {
                        sleep(duration);
                        Ok(true)
                    }
                    Err(_) => {
                        f();
                        Ok(false)
                    }
                }
            })?;
        }
        Ok(timer)
    }

    fn elapses_at(&self) -> SystemTime {
        *self.at.lock().unwrap()
    }

    pub fn set_at(&self, time: SystemTime) {
        let mut at = self.at.lock().unwrap();
        *at = time;
    }
}
