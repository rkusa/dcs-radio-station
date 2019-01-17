#![feature(try_trait)]
#![warn(rust_2018_idioms)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate const_cstr;

#[macro_use]
mod macros;

use std::ffi::CString;
use std::ptr;

use drsplayer::{Error, Player, Position};
use hlua51::{Lua, LuaFunction, LuaTable};
use libc::c_int;
use lua51_sys as ffi;

static mut INITIALIZED: bool = false;
static mut PLAYER: Option<Player> = None;

pub fn init(lua: &mut Lua<'_>) -> Result<(), Error> {
    unsafe {
        if INITIALIZED {
            return Ok(());
        }
        INITIALIZED = true;
    }

    // init logging
    use log::LevelFilter;
    use log4rs::append::console::ConsoleAppender;
    use log4rs::append::file::FileAppender;
    use log4rs::config::{Appender, Config, Logger, Root};

    let config = if let Some(mut lfs) = lua.get::<LuaTable<_>, _>("lfs") {
        let mut writedir: LuaFunction<_> = get!(lfs, "writedir")?;
        let writedir: String = writedir.call()?;
        let log_file = writedir + "Logs/drs.log";

        let requests = FileAppender::builder()
            .append(false)
            .build(log_file)
            .unwrap();

        Config::builder()
            .appender(Appender::builder().build("file", Box::new(requests)))
            .logger(Logger::builder().build("drs", LevelFilter::Debug))
            .build(Root::builder().appender("file").build(LevelFilter::Off))
            .unwrap()
    } else {
        let stdout = ConsoleAppender::builder().build();
        Config::builder()
            .appender(Appender::builder().build("stdout", Box::new(stdout)))
            .logger(Logger::builder().build("drs", LevelFilter::Debug))
            .build(Root::builder().appender("stdout").build(LevelFilter::Off))
            .unwrap()
    };

    log4rs::init_config(config).unwrap();

    Ok(())
}

#[no_mangle]
pub extern "C" fn start(state: *mut ffi::lua_State) -> c_int {
    unsafe {
        if PLAYER.is_none() {
            let mut lua = Lua::from_existing_state(state, false);
            let path: String = match lua.pop() {
                Some(p) => p,
                None => {
                    return report_error(state, "path argument required");
                }
            };

            if let Err(err) = init(&mut lua) {
                return report_error(state, &err.to_string());
            }

            info!(
                "Starting SRS Player version {} ...",
                env!("CARGO_PKG_VERSION")
            );

            match Player::create(lua) {
                Ok(player) => {
                    PLAYER = Some(player);
                }
                Err(err) => {
                    return report_error(state, &err.to_string());
                }
            }
        }
    }

    0
}

#[no_mangle]
pub extern "C" fn stop(_state: *mut ffi::lua_State) -> c_int {
    unsafe {
        if let Some(player) = PLAYER.take() {
            info!("Stopping ...");
            player.stop()
        }
    }

    0
}

#[no_mangle]
pub extern "C" fn pause(_state: *mut ffi::lua_State) -> c_int {
    unsafe {
        if let Some(ref mut player) = PLAYER {
            debug!("Pausing ...");
            player.pause()
        }
    }

    0
}

#[no_mangle]
pub extern "C" fn unpause(_state: *mut ffi::lua_State) -> c_int {
    unsafe {
        if let Some(ref mut player) = PLAYER {
            debug!("Unpausing ...");
            player.unpause()
        }
    }

    0
}

fn report_error(state: *mut ffi::lua_State, msg: &str) -> c_int {
    let msg = CString::new(msg).unwrap();

    unsafe {
        ffi::lua_pushstring(state, msg.as_ptr());
        ffi::lua_error(state);
    }

    0
}

#[no_mangle]
#[allow(non_snake_case)]
pub unsafe extern "C" fn luaopen_drs(state: *mut ffi::lua_State) -> c_int {
    let registration = &[
        ffi::luaL_Reg {
            name: cstr!("start"),
            func: Some(start),
        },
        ffi::luaL_Reg {
            name: cstr!("stop"),
            func: Some(stop),
        },
        ffi::luaL_Reg {
            name: cstr!("pause"),
            func: Some(pause),
        },
        ffi::luaL_Reg {
            name: cstr!("unpause"),
            func: Some(unpause),
        },
        ffi::luaL_Reg {
            name: ptr::null(),
            func: None,
        },
    ];

    ffi::luaL_openlib(state, cstr!("drs"), registration.as_ptr(), 0);

    1
}
