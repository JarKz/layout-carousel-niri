use std::{error::Error, path::PathBuf, time::UNIX_EPOCH};

use clap::{CommandFactory, Parser};
use clap_complete::{Shell, generate};
use directories::BaseDirs;
use niri_ipc::{Request, Response, socket::Socket};
use serde::{Deserialize, Serialize};

type CarouselResult<T> = Result<T, Box<dyn Error>>;

#[derive(Debug, derive_more::Display)]
enum CarouselError {
    #[display(
        "You're running this application either as root or as another user that don't have home directory."
    )]
    InvalidRun,
    #[display("There's something wrong with niri IPC. Check your application and niri version.")]
    IpcProblems,
    #[display(
        "Invalid passed max duration to set. Required to be in range [0.2; 1.0], but given {max_duration}"
    )]
    IncorrectMaxDuration { max_duration: Duration },
}

impl Error for CarouselError {}

/// The layout carousel for niri WM. Switches layouts in comfort way like MacOS.
#[derive(Parser)]
enum LayoutCarouselCmd {
    /// Switches to last used in single call, but uses next in double or more calls.
    Switch,
    /// The maximum duration between two calls to pick the next after last used layout.
    KeypressDuration { duration: Option<f64> },
    /// Resetting all settings to default according to niri config file.
    Reload,
    /// Prints the completion code for a specific shell.
    Completion { shell: Option<Shell> },
}

#[derive(Serialize, Deserialize)]
struct CarouselData {
    last_time: f64,
    layouts: Vec<usize>,
    index_frequent: usize,
    index_rotational: usize,
    sum_time: f64,
    counter: u8,

    #[serde(default)]
    max_duration: Duration,
}

impl CarouselData {
    fn create_default(socket: &mut Socket) -> CarouselResult<Self> {
        let Response::KeyboardLayouts(layouts) = socket.send(Request::KeyboardLayouts)?? else {
            return Err(Box::new(CarouselError::IpcProblems));
        };

        Ok(CarouselData {
            last_time: std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time after UNIX epoch")
                .as_secs_f64(),
            counter: 0,
            layouts: (0..layouts.names.len()).collect(),
            index_frequent: 0,
            index_rotational: 0,
            sum_time: 0.0,
            max_duration: Duration::default(),
        })
    }

    fn load() -> CarouselResult<Self> {
        Ok(serde_json::from_str(&std::fs::read_to_string(
            Self::get_path(false)?,
        )?)?)
    }

    fn dump(&self) -> CarouselResult<()> {
        Ok(std::fs::write(
            Self::get_path(true)?,
            serde_json::to_string(self)?,
        )?)
    }

    fn get_path(create_directory: bool) -> CarouselResult<PathBuf> {
        let mut data_directory = BaseDirs::new()
            .ok_or(CarouselError::InvalidRun)?
            .data_dir()
            .to_path_buf();
        data_directory.push("layout-carousel-niri");
        if !data_directory.exists() && create_directory {
            std::fs::create_dir_all(&data_directory)?;
        }
        data_directory.push("data");
        Ok(data_directory)
    }

    fn compute_time_and_count(&mut self, call_time: f64) {
        let diff = call_time - self.last_time;
        self.last_time = call_time;

        self.sum_time += diff;
        if self.max_duration.satisfies(self.sum_time) {
            self.counter += 1;
        } else {
            self.sum_time = 0.0;
            self.counter = 1;
        }

        if self.counter > 1 {
            self.sum_time = 0.0;
        }
    }

    fn handle_switch(&mut self) {
        if self.counter <= 1 {
            self.index_frequent = (self.index_frequent + 1) % 2;
        } else {
            if self.counter > 2 {
                self.index_rotational += 1;
            } else {
                // INFO: need to turn back to previous layout to switch it to any picked by user.
                self.index_frequent = (self.index_frequent + 1) % 2;
                self.index_rotational = 2;
            }

            self.index_rotational %= self.layouts.len();

            self.layouts
                .swap(self.index_frequent, self.index_rotational);
        }
    }
}

#[derive(Serialize, Deserialize, derive_more::Display, Debug)]
#[display("{_0}")]
struct Duration(f64);

impl Duration {
    const DEFAULT_MAX_DURATION: f64 = 0.4;
    const MIN: f64 = 0.2;
    const MAX: f64 = 1.0;

    fn satisfies(&self, time: f64) -> bool {
        time < self.0
    }

    fn within_range(&self) -> bool {
        (Self::MIN..Self::MAX).contains(&self.0)
    }
}

impl Default for Duration {
    fn default() -> Self {
        Self(Self::DEFAULT_MAX_DURATION)
    }
}

impl LayoutCarouselCmd {
    fn handle(&mut self) -> CarouselResult<()> {
        let mut socket = Socket::connect()?;
        match self {
            LayoutCarouselCmd::Switch => handle_layout_switch(&mut socket),
            LayoutCarouselCmd::KeypressDuration { duration } => {
                handle_keypress_duration(&mut socket, duration)
            }
            LayoutCarouselCmd::Reload => CarouselData::create_default(&mut socket)?.dump(),
            LayoutCarouselCmd::Completion { shell } => {
                let mut command = LayoutCarouselCmd::command();
                let name = command.get_name().to_string();
                generate(
                    shell.unwrap_or(Shell::Bash),
                    &mut command,
                    name,
                    &mut std::io::stdout(),
                );
                Ok(())
            }
        }
    }
}

fn handle_layout_switch(socket: &mut Socket) -> CarouselResult<()> {
    let call_time = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time after UNIX epoch")
        .as_secs_f64();
    let mut data = CarouselData::load().or_else(|_| CarouselData::create_default(socket))?;

    // INFO: check is single layout in system to avoid useless computations.
    if data.layouts.len() < 2 {
        return Ok(());
    }

    data.compute_time_and_count(call_time);
    data.handle_switch();

    socket.send(Request::Action(niri_ipc::Action::SwitchLayout {
        layout: niri_ipc::LayoutSwitchTarget::Index(data.layouts[data.index_frequent] as u8),
    }))??;
    data.dump()
}

fn handle_keypress_duration(socket: &mut Socket, duration: &mut Option<f64>) -> CarouselResult<()> {
    let mut data = CarouselData::load().or_else(|_| CarouselData::create_default(socket))?;
    match duration {
        None => {
            println!("Current max keypress duration: {}", data.max_duration);
            Ok(())
        }
        Some(new_duration) => {
            let new_max_duration = Duration(*new_duration);
            if !new_max_duration.within_range() {
                return Err(Box::new(CarouselError::IncorrectMaxDuration {
                    max_duration: new_max_duration,
                }));
            }
            data.max_duration = new_max_duration;
            data.dump()
        }
    }
}

fn main() -> CarouselResult<()> {
    LayoutCarouselCmd::parse().handle()
}
