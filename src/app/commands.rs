use super::App;
use crate::core::{InputCommand, NdiInputState, SharedState};
#[cfg(feature = "ndi")]
use crate::core::NdiOutputCommand;
use std::sync::Arc;

/// Acquire a mutex lock, recovering from poisoning.
fn lock(state: &std::sync::Mutex<SharedState>) -> std::sync::MutexGuard<SharedState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

impl App {
    /// Dispatch all pending commands. Call once per frame.
    pub(super) fn dispatch_commands(&mut self) {
        self.process_input_commands();
        #[cfg(feature = "ndi")]
        self.process_mapping_output_commands();
        #[cfg(feature = "ndi")]
        self.process_matrix_output_commands();
        self.process_syphon_output_commands();
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
            #[cfg(feature = "ndi")]
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
            #[cfg(feature = "ndi")]
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

    #[cfg(feature = "ndi")]
    fn process_mapping_output_commands(&mut self) {
        let command = {
            let mut state = lock(&self.shared_state);
            std::mem::replace(&mut state.mapping_ndi_output_command, NdiOutputCommand::None)
        };

        match command {
            NdiOutputCommand::Start => {
                let (name, include_alpha) = {
                    let state = lock(&self.shared_state);
                    (state.mapping_ndi_output.stream_name.clone(), state.mapping_ndi_output.include_alpha)
                };
                if let Some(ref mut output) = self.mapping_output {
                    let (w, h) = {
                        let size = output.inner_size();
                        (size.width.max(1), size.height.max(1))
                    };
                    if let Err(e) = output.start_ndi(&name, w, h, include_alpha) {
                        log::error!("Failed to start mapping NDI output: {:?}", e);
                    } else {
                        lock(&self.shared_state).mapping_ndi_output.is_active = true;
                    }
                }
            }
            NdiOutputCommand::Stop => {
                if let Some(ref mut output) = self.mapping_output {
                    output.stop_ndi();
                }
                lock(&self.shared_state).mapping_ndi_output.is_active = false;
            }
            NdiOutputCommand::None => {}
        }
    }

    #[cfg(feature = "ndi")]
    fn process_matrix_output_commands(&mut self) {
        let command = {
            let mut state = lock(&self.shared_state);
            std::mem::replace(&mut state.matrix_ndi_output_command, NdiOutputCommand::None)
        };

        match command {
            NdiOutputCommand::Start => {
                let (name, include_alpha) = {
                    let state = lock(&self.shared_state);
                    (state.matrix_ndi_output.stream_name.clone(), state.matrix_ndi_output.include_alpha)
                };
                if let Some(ref mut output) = self.matrix_output {
                    let (w, h) = {
                        let size = output.inner_size();
                        (size.width.max(1), size.height.max(1))
                    };
                    if let Err(e) = output.start_ndi(&name, w, h, include_alpha) {
                        log::error!("Failed to start matrix NDI output: {:?}", e);
                    } else {
                        lock(&self.shared_state).matrix_ndi_output.is_active = true;
                    }
                }
            }
            NdiOutputCommand::Stop => {
                if let Some(ref mut output) = self.matrix_output {
                    output.stop_ndi();
                }
                lock(&self.shared_state).matrix_ndi_output.is_active = false;
            }
            NdiOutputCommand::None => {}
        }
    }

    /// Process Syphon output start/stop for both windows (macOS).
    fn process_syphon_output_commands(&mut self) {
        #[cfg(target_os = "macos")]
        {
            // Mapping Syphon
            let (mapping_enabled, mapping_name) = {
                let state = lock(&self.shared_state);
                (state.mapping_syphon_output.enabled, state.mapping_syphon_output.server_name.clone())
            };
            if let Some(ref mut output) = self.mapping_output {
                let should_be_active = mapping_enabled;
                let is_active = output.is_syphon_active();
                if should_be_active && !is_active {
                    if let (Some(device), Some(queue)) = (&self.wgpu_device, &self.wgpu_queue) {
                        if let Err(e) = output.start_syphon(&mapping_name, Arc::clone(device), Arc::clone(queue)) {
                            log::error!("Failed to start mapping Syphon output: {:?}", e);
                        }
                    }
                } else if !should_be_active && is_active {
                    output.stop_syphon();
                }
            }

            // Matrix Syphon
            let (matrix_enabled, matrix_name) = {
                let state = lock(&self.shared_state);
                (state.matrix_syphon_output.enabled, state.matrix_syphon_output.server_name.clone())
            };
            if let Some(ref mut output) = self.matrix_output {
                let should_be_active = matrix_enabled;
                let is_active = output.is_syphon_active();
                if should_be_active && !is_active {
                    if let (Some(device), Some(queue)) = (&self.wgpu_device, &self.wgpu_queue) {
                        if let Err(e) = output.start_syphon(&matrix_name, Arc::clone(device), Arc::clone(queue)) {
                            log::error!("Failed to start matrix Syphon output: {:?}", e);
                        }
                    }
                } else if !should_be_active && is_active {
                    output.stop_syphon();
                }
            }
        }
    }
}

/// Return a mutable reference to the input state for the given slot.
fn input_state_mut(state: &mut SharedState, slot: u8) -> &mut crate::core::NdiInputState {
    if slot == 1 { &mut state.ndi_input1 } else { &mut state.ndi_input2 }
}
