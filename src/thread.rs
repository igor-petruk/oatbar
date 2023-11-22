// Copyright 2023 Oatbar Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::Context;
use tracing::*;

pub fn spawn<S, F>(name: S, f: F) -> anyhow::Result<()>
where
    S: Into<String>,
    F: FnOnce() -> anyhow::Result<()> + Send + 'static,
{
    let name: String = name.into();
    let context_name = name.clone();
    std::thread::Builder::new()
        .name(name)
        .spawn(move || {
            trace!("Thread started.");
            let result = f();
            match &result {
                Ok(_) => {
                    trace!("Thread finished: {:?}.", result);
                }
                Err(_) => {
                    error!("Thread finished: {:?}", result);
                }
            }
        })
        .with_context(move || format!("failed to spawn thread: {}", context_name))?;

    Ok(())
}

pub fn spawn_loop<S, F>(name: S, mut f: F) -> anyhow::Result<()>
where
    S: Into<String>,
    F: FnMut() -> anyhow::Result<bool> + Send + 'static,
{
    spawn(name, move || loop {
        let keep_running = f()?;
        if !keep_running {
            return Ok(());
        }
    })?;
    Ok(())
}
