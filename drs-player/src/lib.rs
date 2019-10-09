#![warn(rust_2018_idioms)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

#[macro_use]
mod macros;
mod error;
mod worker;

use std::io::{self, BufRead, BufReader, Cursor, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{fmt, thread};

pub use crate::error::Error;
use crate::worker::{Context, Worker};
use byteorder::{LittleEndian, WriteBytesExt};
use hlua51::{Lua, LuaFunction, LuaTable};
use uuid::Uuid;
use either::Either;
use audiopus::{coder::Encoder, Application, Channels, SampleRate};

const MAX_FRAME_LENGTH: usize = 1024;

pub struct Player {
    sguid: String,
    worker: Vec<Worker<()>>,
    name: String,
    position: Position,
    freq: u64,
}

struct AudioFile {
    path: PathBuf,
    #[allow(unused)]
    duration: Duration,
}

impl Player {
    pub fn new(name: &str, position: Position, freq: u64) -> Self {
        let sguid = Uuid::new_v4();
        let sguid = base64::encode_config(sguid.as_bytes(), base64::URL_SAFE_NO_PAD);
        assert_eq!(sguid.len(), 22);

        Player {
            sguid,
            worker: Vec::new(),
            name: name.to_string(),
            position,
            freq,
        }
    }

    pub fn create(mut lua: Lua<'_>) -> Result<Self, Error> {
        debug!("Extracting ATIS stations from Mission Situation");

        // extract all mission statics to later look for ATIS configs in their names
        let mut comm_towers = {
            // `_current_mission.mission.coalition.{blue,red}.country[i].static.group[j]
            let mut current_mission: LuaTable<_> = get!(lua, "_current_mission")?;
            let mut mission: LuaTable<_> = get!(current_mission, "mission")?;
            let mut coalitions: LuaTable<_> = get!(mission, "coalition")?;

            let mut comm_towers = Vec::new();
            let keys = vec!["blue", "red"];
            for key in keys {
                let mut coalition: LuaTable<_> = get!(coalitions, key)?;
                let mut countries: LuaTable<_> = get!(coalition, "country")?;

                let mut i = 1;
                while let Some(mut country) = countries.get::<LuaTable<_>, _, _>(i) {
                    if let Some(mut statics) = country.get::<LuaTable<_>, _, _>("static") {
                        if let Some(mut groups) = statics.get::<LuaTable<_>, _, _>("group") {
                            let mut j = 1;
                            while let Some(mut group) = groups.get::<LuaTable<_>, _, _>(j) {
                                let x: f64 = get!(group, "x")?;
                                let y: f64 = get!(group, "y")?;

                                // read `group.units[1].unitId
                                let mut units: LuaTable<_> = get!(group, "units")?;
                                let mut first_unit: LuaTable<_> = get!(units, 1)?;
                                let unit_id: i32 = get!(first_unit, "unitId")?;

                                comm_towers.push(CommTower {
                                    id: unit_id,
                                    name: String::new(),
                                    x,
                                    y,
                                    alt: 0.0,
                                });

                                j += 1;
                            }
                        }
                    }
                    i += 1;
                }
            }
            comm_towers
        };

        // extract the names for all statics
        {
            // read `DCS.getUnitProperty`
            let mut dcs: LuaTable<_> = get!(lua, "DCS")?;
            let mut get_unit_property: LuaFunction<_> = get!(dcs, "getUnitProperty")?;
            for mut tower in &mut comm_towers {
                // 3 = DCS.UNIT_NAME
                tower.name = get_unit_property.call_with_args((tower.id, 3))?;
            }
        }

        // read the terrain height for all airdromes and statics
        {
            // read `Terrain.GetHeight`
            let mut terrain: LuaTable<_> = get!(lua, "Terrain")?;
            let mut get_height: LuaFunction<_> = get!(terrain, "GetHeight")?;

            for mut tower in &mut comm_towers {
                tower.alt = get_height.call_with_args((tower.x, tower.y))?;
            }
        }

        let mut station = comm_towers
            .into_iter()
            .filter_map(|tower| {
                if tower.name == "SRS Player" {
                    Some(tower)
                } else {
                    None
                }
            })
            .next();

        if let Some(station) = station.take() {
            Ok(Player::new(
                "SRS Radio",
                Position {
                    x: station.x,
                    y: station.y,
                    alt: station.alt,
                },
                255_000_000,
            ))
        } else {
            Err(Error::NoStationFound)
        }
    }

    pub fn start<P: AsRef<Path>>(mut self, path: P, should_loop: bool) -> Result<(), Error> {
        if self.worker.len() > 0 {
            // already started
            return Ok(());
        }

        let file_paths: Vec<PathBuf> = if path.as_ref().is_dir() {
            path.as_ref().read_dir()?.filter_map(|entry| {
                entry.ok().map(|e| e.path())
            }).collect()
        } else {
            vec![path.as_ref().into()]
        };

        let mut audio_files = Vec::new();
        for path in file_paths {
            if path.extension().is_none() || path.extension().unwrap() != "wav" {
                warn!("Ignoring non .wav file: {:?}", path);
                continue;
            }
            match hound::WavReader::open(&path) {
                Ok(wav) => {
                    let spec = wav.spec();
                    audio_files.push(AudioFile { path, duration: Duration::from_secs(u64::from(wav.duration() / spec.sample_rate)) });
                }
                Err(err) => {
                    error!("reading wav file {} failed with: {}", path.to_string_lossy(), err);
                }
            }
        }

        let mut stream = TcpStream::connect("127.0.0.1:5002")?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;

        let name = format!("ATIS {}", self.name);
        let sync_msg = Message {
            client: Some(Client {
                client_guid: &self.sguid,
                name: &name,
                position: self.position.clone(),
                coalition: Coalition::Blue,
                radio_info: Some(RadioInfo {
                    name: "ATIS",
                    pos: self.position.clone(),
                    ptt: false,
                    radios: vec![Radio {
                        enc: false,
                        enc_key: 0,
                        enc_mode: 0, // no encryption
                        freq_max: 1.0,
                        freq_min: 1.0,
                        freq: self.freq as f64,
                        modulation: 0,
                        name: "ATIS",
                        sec_freq: 0.0,
                        volume: 1.0,
                        freq_mode: 0, // Cockpit
                        vol_mode: 0,  // Cockpit
                        expansion: false,
                        channel: -1,
                        simul: false,
                    }],
                    control: 0, // HOTAS
                    selected: 0,
                    unit: &name,
                    unit_id: 0,
                    simultaneous_transmission: true,
                }),
            }),
            msg_type: MsgType::Sync,
            version: "1.6.0.0",
        };

        serde_json::to_writer(&stream, &sync_msg)?;
        stream.write_all(&['\n' as u8])?;

        let mut rd = BufReader::new(stream.try_clone().unwrap()); // TODO: unwrap?

        // spawn thread that sends an update RPC call to SRS every ~5 seconds
        let sguid = self.sguid.clone();
        let mut position = self.position.clone();
        position.alt += 100.0; // increase sending alt to 100ft above ground for LOS
        self.worker.push(Worker::new(move |ctx| {
            let mut send_update = || -> Result<(), Error> {
                // send update
                let upd_msg = Message {
                    client: Some(Client {
                        client_guid: &sguid,
                        name: &name,
                        position: position.clone(),
                        coalition: Coalition::Blue,
                        radio_info: None,
                    }),
                    msg_type: MsgType::Update,
                    version: "1.5.6.0",
                };

                serde_json::to_writer(&mut stream, &upd_msg)?;
                stream.write_all(&['\n' as u8])?;

                Ok(())
            };

            loop {
                if let Err(err) = send_update() {
                    error!("Error sending update to SRS: {}", err);
                }

                //                debug!("SRS Update sent");

                if ctx.should_stop_timeout(Duration::from_secs(5)) {
                    return ();
                }
            }
        }));

        self.worker.push(Worker::new(move |ctx| {
            let mut data = Vec::new();

            loop {
                match rd.read_until(b'\n', &mut data) {
                    Ok(bytes_read) => {
                        if bytes_read == 0 {
                            return ();
                        }

                        data.clear();
                        // ignore received messages ...
                    }
                    Err(err) => match err.kind() {
                        io::ErrorKind::TimedOut => {}
                        _ => {
                            error!(
                                "Error ({:?}) receiving update from SRS: {}",
                                err.kind(),
                                err
                            );
                        }
                    },
                }

                if ctx.should_stop() {
                    return ();
                }
            }
        }));

        // run audio broadcast
        let sguid = self.sguid.clone();
        let freq = self.freq;
        let broadcast_worker = Worker::new(move |ctx| {
            if let Err(err) = audio_broadcast(ctx, sguid, freq, audio_files, should_loop) {
                error!("Error starting SRS broadcast: {}", err);
            }
        });
        // self.worker.push(broadcast_worker);
        broadcast_worker.join();

        // if we looping, we will never reach this position, if we aren't looping, stop
        // all other workers since we are done
        self.stop();

        Ok(())
    }

    pub fn stop(self) {
        for worker in self.worker.into_iter() {
            worker.stop();
        }
    }

    pub fn pause(&self) {
        for worker in &self.worker {
            worker.pause();
        }
    }

    pub fn unpause(&self) {
        for worker in &self.worker {
            worker.unpause();
        }
    }
}

struct CommTower {
    id: i32,
    name: String,
    x: f64,
    y: f64,
    alt: f64,
}

fn audio_broadcast(
    ctx: Context,
    sguid: String,
    freq: u64,
    files: Vec<AudioFile>,
    should_loop: bool,
) -> Result<(), Error> {
    let mut stream = TcpStream::connect("127.0.0.1:5003")?;
    stream.set_nodelay(true)?;

    let iter = if  should_loop {
        Either::Left(files.iter().cycle())
    } else {
        Either::Right(files.iter())
    };
    for AudioFile { ref path, .. } in iter {
        debug!("Playing {}", path.to_string_lossy());

        let start = Instant::now();
        let mut size = 0;
        let mut id: u64 = 1;

        let mut wav =  hound::WavReader::open(path)?;
        // Note: SampleRate::Hz16000 didn't work
        let enc = Encoder::new(SampleRate::Hz24000, Channels::Mono, Application::Voip)?;

        // e.g. 24000Hz * 1 channel * 20 ms / 1000
        const MONO_20MS: usize = 24000 * 1 * 20 / 1000;
        let mut buffer = [0_i16; MONO_20MS];

        // Note:The following didn't work
        // let spec = wav.spec();
        // let mono20ms = spec.sample_rate * u32::from(spec.channels) * 20 / 1000;
        // let mut buffer = Vec::with_capacity(mono20ms);

        let mut i = 0;
        let mut output = [0; 256];

        for s in wav.samples::<i16>() {
            if i >= buffer.len() {
                let len = enc
                    .encode(&buffer, &mut output)?;
                
                size += len;
                        
                let frame = pack_frame(&sguid, id, freq, &output[..len])?;
                stream.write(&frame)?;
                id += 1;

                // 32 kBit/s
                let secs = (size * 8) as f64 / 1024.0 / 32.0;

                let playtime = Duration::from_millis((secs * 1000.0) as u64);
                let elapsed = Instant::now() - start;
                if playtime > elapsed {
                    thread::sleep(playtime - elapsed);
                }

                if ctx.should_stop() {
                    return Ok(());
                }

                i = 0;
            }

            let s = s.unwrap();
            buffer[i] = s;

            i += 1;
        } 

        debug!("TOTAL SIZE: {}", size);

        // 32 kBit/s
        let secs = (size * 8) as f64 / 1024.0 / 32.0;
        debug!("SECONDS: {}", secs);

        let playtime = Duration::from_millis((secs * 1000.0) as u64);
        let elapsed = Instant::now() - start;
        if playtime > elapsed {
            thread::sleep(playtime - elapsed);
        }

        if ctx.should_stop_timeout(Duration::from_secs(3)) {
            return Ok(());
        }
    }

    Ok(())
}

fn pack_frame(sguid: &str, id: u64, freq: u64, rd: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut frame = Cursor::new(Vec::with_capacity(MAX_FRAME_LENGTH));

    // header segment will be written at the end
    frame.set_position(6);

    // - AUDIO SEGMENT
    let len_before = frame.position();
    // AudioPart1
    frame.write_all(&rd)?;
    let len_audio_part = frame.position() - len_before;

    // - FREQUENCY SEGMENT
    let len_before = frame.position();
    // Frequency
    frame.write_f64::<LittleEndian>(freq as f64)?;
    // Modulation
    //    AM = 0,
    //    FM = 1,
    //    INTERCOM = 2,
    //    DISABLED = 3
    frame.write_all(&[0])?;
    // Encryption
    //    NO_ENCRYPTION = 0,
    //    ENCRYPTION_JUST_OVERLAY = 1,
    //    ENCRYPTION_FULL = 2,
    //    ENCRYPTION_COCKPIT_TOGGLE_OVERLAY_CODE = 3
    frame.write_all(&[0])?;
    let len_frequency = frame.position() - len_before;

    // - FIXED SEGMENT
    // UnitId
    frame.write_u32::<LittleEndian>(0)?;
    // PacketId
    frame.write_u64::<LittleEndian>(id)?;
    // GUID
    frame.write_all(sguid.as_bytes())?;

    // - HEADER SEGMENT
    let len_packet = frame.get_ref().len();
    frame.set_position(0);
    // Packet Length
    frame.write_u16::<LittleEndian>(len_packet as u16)?;
    // AudioPart1 Length
    frame.write_u16::<LittleEndian>(len_audio_part as u16)?;
    // FrequencyPart Length
    frame.write_u16::<LittleEndian>(len_frequency as u16)?;

    Ok(frame.into_inner())
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: f64,
    #[serde(rename = "z")]
    pub y: f64,
    #[serde(rename = "y")]
    pub alt: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MsgType {
    Update,
    Sync,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Coalition {
    Blue,
    Red,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Radio<'a> {
    enc: bool,
    enc_key: u8,
    enc_mode: u8,
    freq_max: f64,   // 1.0,
    freq_min: f64,   // 1.0,
    freq: f64,       // 1.0,
    modulation: u8,  // 3,
    name: &'a str,   // "No Radio",
    sec_freq: f64,   // 0.0,
    volume: f32,     // 1.0,
    freq_mode: u8,   // 0,
    vol_mode: u8,    // 0,
    expansion: bool, // false,
    channel: i32,    // -1,
    simul: bool,     // false
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RadioInfo<'a> {
    name: &'a str,
    pos: Position,
    ptt: bool,
    radios: Vec<Radio<'a>>,
    control: u8,
    selected: usize,
    unit: &'a str,
    unit_id: usize,
    simultaneous_transmission: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct Client<'a> {
    client_guid: &'a str,
    name: &'a str,
    position: Position,
    coalition: Coalition,
    radio_info: Option<RadioInfo<'a>>,
    // ClientChannelId
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
struct Message<'a> {
    client: Option<Client<'a>>,
    msg_type: MsgType,
    // Clients
    // ServerSettings
    // ExternalAWACSModePassword
    version: &'a str,
}

impl ::serde::Serialize for MsgType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        // Serialize the enum as a u64.
        serializer.serialize_u64(match *self {
            MsgType::Update => 1,
            MsgType::Sync => 2,
        })
    }
}

impl<'de> ::serde::Deserialize<'de> for MsgType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> ::serde::de::Visitor<'de> for Visitor {
            type Value = MsgType;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("positive integer")
            }

            fn visit_u64<E>(self, value: u64) -> Result<MsgType, E>
            where
                E: ::serde::de::Error,
            {
                // Rust does not come with a simple way of converting a
                // number to an enum, so use a big `match`.
                match value {
                    1 => Ok(MsgType::Update),
                    2 => Ok(MsgType::Sync),
                    _ => Err(E::custom(format!(
                        "unknown {} value: {}",
                        stringify!(MsgType),
                        value
                    ))),
                }
            }
        }

        // Deserialize the enum from a u64.
        deserializer.deserialize_u64(Visitor)
    }
}

impl ::serde::Serialize for Coalition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        // Serialize the enum as a u64.
        serializer.serialize_u64(match *self {
            Coalition::Blue => 2,
            Coalition::Red => 1,
        })
    }
}

impl<'de> ::serde::Deserialize<'de> for Coalition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> ::serde::de::Visitor<'de> for Visitor {
            type Value = Coalition;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("positive integer")
            }

            fn visit_u64<E>(self, value: u64) -> Result<Coalition, E>
            where
                E: ::serde::de::Error,
            {
                // Rust does not come with a simple way of converting a
                // number to an enum, so use a big `match`.
                match value {
                    1 => Ok(Coalition::Red),
                    2 => Ok(Coalition::Blue),
                    _ => Err(E::custom(format!(
                        "unknown {} value: {}",
                        stringify!(Coalition),
                        value
                    ))),
                }
            }
        }

        // Deserialize the enum from a u64.
        deserializer.deserialize_u64(Visitor)
    }
}
