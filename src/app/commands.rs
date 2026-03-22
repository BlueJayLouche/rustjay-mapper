use super::App;
use crate::core::{InputCommand, NdiOutputCommand, NdiInputState, SharedState};

/// Acquire a mutex lock, recovering from poisoning.
fn lock(state: &std::sync::Mutex<SharedState>) -> std::sync::MutexGuard<SharedState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

impl App {
    /// Dispatch all pending commands. Call once per frame.
    pub(super) fn dispatch_commands(&mut self) {
        self.process_input_commands();
        self.process_output_commands();
    }

    fn process_input_commands(&mut self) {
        let (cmd1, cmd2) = {
            let mut state = lock(&self.shared_state);
            let c1 = std::mem::replace(&mut state.input1_command, InputCommand::None);
            let c2 = std::mem::replace(&mut state.input2_command, InputCommand::None);
            (c1, c2)
        };

        self.apply_input_command(1, cmd1);
        self.apply_input_command(2, cmd2);
    }

    /// Apply a single input command for the given slot (1 or 2).
    fn apply_input_command(&mut self, slot: u8, cmd: InputCommand) {
        match cmd {
            InputCommand::StartWebcam { device_index, width, height, fps } => {
                log::info!("Starting webcam on input {}: device={}", slot, device_index);
                let result = if let Some(ref mut manager) = self.input_manager {
                    Some(if slot == 1 {
                        manager.start_input1_webcam(device_index, width, height, fps)
                    } else {
                        manager.start_input2_webcam(device_index, width, height, fps)
                    })
                } else {
                    None
                };
                match result {
                    Some(Ok(_)) => {
                        let mut state = lock(&self.shared_state);
                        let inp = input_state_mut(&mut state, slot);
                        inp.is_active = true;
                        inp.source_name = format!("Webcam {}", device_index);
                    }
                    Some(Err(e)) => log::error!("Failed to start webcam on input {}: {:?}", slot, e),
                    None => {}
                }
            }
            InputCommand::StartNdi { source_name } => {
                log::info!("Starting NDI on input {}: {}", slot, source_name);
                let result = if let Some(ref mut manager) = self.input_manager {
                    Some(if slot == 1 {
                        manager.start_input1_ndi(&source_name)
                    } else {
                        manager.start_input2_ndi(&source_name)
                    })
                } else {
                    None
                };
                match result {
                    Some(Ok(_)) => {
                        let mut state = lock(&self.shared_state);
                        let inp = input_state_mut(&mut state, slot);
                        inp.is_active = true;
                        inp.source_name = source_name;
                    }
                    Some(Err(e)) => log::error!("Failed to start NDI on input {}: {:?}", slot, e),
                    None => {}
                }
            }
            InputCommand::StartObs { source_name } => {
                log::info!("Starting OBS on input {}: {}", slot, source_name);
                let result = if let Some(ref mut manager) = self.input_manager {
                    Some(if slot == 1 {
                        manager.start_input1_obs(&source_name)
                    } else {
                        manager.start_input2_obs(&source_name)
                    })
                } else {
                    None
                };
                match result {
                    Some(Ok(_)) => {
                        let mut state = lock(&self.shared_state);
                        let inp = input_state_mut(&mut state, slot);
                        inp.is_active = true;
                        inp.source_name = source_name;
                    }
                    Some(Err(e)) => log::error!("Failed to start OBS on input {}: {:?}", slot, e),
                    None => {}
                }
            }
            #[cfg(target_os = "macos")]
            InputCommand::StartSyphon { server_name } => {
                log::info!("Starting Syphon on input {}: {}", slot, server_name);
                let result = if let Some(ref mut manager) = self.input_manager {
                    Some(if slot == 1 {
                        manager.start_input1_syphon(&server_name)
                    } else {
                        manager.start_input2_syphon(&server_name)
                    })
                } else {
                    None
                };
                match result {
                    Some(Ok(_)) => {
                        let mut state = lock(&self.shared_state);
                        let inp = input_state_mut(&mut state, slot);
                        inp.is_active = true;
                        inp.source_name = server_name;
                    }
                    Some(Err(e)) => log::error!("Failed to start Syphon on input {}: {:?}", slot, e),
                    None => {}
                }
            }
            InputCommand::StopInput => {
                if let Some(ref mut manager) = self.input_manager {
                    if slot == 1 {
                        manager.stop_input1();
                    } else {
                        manager.stop_input2();
                    }
                }
                let mut state = lock(&self.shared_state);
                let inp = input_state_mut(&mut state, slot);
                inp.is_active = false;
                inp.source_name.clear();
            }
            InputCommand::RefreshDevices => {
                if let Some(ref mut manager) = self.input_manager {
                    manager.kick_discovery();
                }
            }
            InputCommand::None => {}
        }
    }

    fn process_output_commands(&mut self) {
        let command = {
            let mut state = lock(&self.shared_state);
            std::mem::replace(&mut state.ndi_output_command, NdiOutputCommand::None)
        };

        match command {
            NdiOutputCommand::Start => {
                if self.ndi_output.is_none() {
                    let (name, include_alpha) = {
                        let state = lock(&self.shared_state);
                        (state.ndi_output.stream_name.clone(), state.ndi_output.include_alpha)
                    };
                    if let Some(ref mut engine) = self.output_engine {
                        if let Err(e) = engine.start_ndi_output(&name, include_alpha, 0) {
                            log::error!("Failed to start NDI output: {:?}", e);
                        } else {
                            lock(&self.shared_state).ndi_output.is_active = true;
                        }
                    }
                }
            }
            NdiOutputCommand::Stop => {
                if let Some(ref mut engine) = self.output_engine {
                    engine.stop_ndi_output();
                }
                lock(&self.shared_state).ndi_output.is_active = false;
            }
            NdiOutputCommand::None => {}
        }
    }
}

/// Return a mutable reference to the input state for the given slot.
fn input_state_mut(state: &mut SharedState, slot: u8) -> &mut crate::core::NdiInputState {
    if slot == 1 { &mut state.ndi_input1 } else { &mut state.ndi_input2 }
}
