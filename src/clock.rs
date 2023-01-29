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

use crate::{state, thread};

pub struct Clock {
    pub format: String,
}

impl state::Source for Clock {
    fn spawn(self, tx: crossbeam_channel::Sender<state::Update>) -> anyhow::Result<()> {
        thread::spawn_loop("clock", move || {
            let time = chrono::Local::now();
            let time_str = time.format(&self.format).to_string();
            tx.send(state::Update {
                entries: vec![state::UpdateEntry {
                    name: "clock".into(),
                    var: "datetime".into(),
                    value: time_str,
                    ..Default::default()
                }],
                ..Default::default()
            })?;
            std::thread::sleep(std::time::Duration::from_secs(5));
            Ok(true)
        })?;
        Ok(())
    }
}
