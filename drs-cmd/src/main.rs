#[macro_use]
extern crate log;

use std::str::FromStr;

use drsplayer::{Error, Player, Position};

pub fn main() -> Result<(), Error> {
    env_logger::Builder::new()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .unwrap();

    let matches = clap::App::new("dcs-radio-station")
        .version(env!("CARGO_PKG_VERSION"))
        .arg(clap::Arg::with_name("frequency")
            .short("f")
            .long("freq")
            .default_value("255000000")
            .help("Sets the SRS frequency (in Hz, e.g. 255000000 for 255MHz)")
            .takes_value(true))
        .arg(clap::Arg::with_name("loop")
            .short("l")
            .long("loop")
            .help("Enables endlessly looping the audio file(s)"))
        .arg(clap::Arg::with_name("PATH")
            .help("Sets the path audio file(s) should be read from")
            .required(true)
            .index(1))
        .get_matches();

    // Calling .unwrap() is safe here because "INPUT" is required
    let path = matches.value_of("PATH").unwrap();
    let should_loop = matches.is_present("loop");
    let freq = matches.value_of("frequency").unwrap();
    let freq = if let Ok(n) = u64::from_str(freq) {
        n
    } else {
        error!("The provided frequency is not a valid number");
        return Ok(());
    };

    let player = Player::new(
        "DCS Radio Station",
        Position {
            x: 0.0,
            y: 0.0,
            alt: 8000.0,
        },
        freq,
    );

    info!("Start playing ...");
    player.start(path, should_loop)?;

    Ok(())
}
