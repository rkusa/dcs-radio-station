use std::{error, fmt};

// TODO: remove ugliness of this workaround ...
type ArgsError =
    ::hlua51::LuaFunctionCallError<::hlua51::TuplePushError<::hlua51::Void, ::hlua51::Void>>;

#[derive(Debug)]
pub enum Error {
    Lua(::hlua51::LuaError),
    GetPluginArgs(ArgsError),
    // TODO: improve by including information about the global/key that was not defined
    Undefined(String),
    Tcp(std::io::Error),
    Json(serde_json::error::Error),
    Request(reqwest::Error),
    Base64Decode(base64::DecodeError),
    Wav(hound::Error),
    Opus(audiopus::Error),
    NoStationFound,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use self::Error::*;
        use std::error::Error;

        match self {
            Undefined(key) => write!(
                f,
                "Error: Trying to access undefined lua global or table key: {}",
                key
            )?,
            _ => write!(f, "Error: {}", self.description())?,
        }

        let mut cause: Option<&dyn error::Error> = self.source();
        while let Some(err) = cause {
            write!(f, "  -> {}", err)?;
            cause = err.source();
        }

        Ok(())
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        use self::Error::*;

        match *self {
            Lua(_) => "Lua error",
            GetPluginArgs(_) => "Error pushing Lua function arguments for OptionsData.getPlugin",
            Undefined(_) => "Trying to access lua gobal or table key that does not exist",
            Tcp(_) => "Error establishing TCP connection to SRS",
            Json(_) => "Error serializing/deserializing JSON RPC message",
            Request(_) => "Error sending TTS request",
            Base64Decode(_) => "Error decoding TTS audio content",
            Wav(_) => "Error reading WAV file",
            Opus(_) => "Error encoding Opus audio stream",
            NoStationFound => "No SRS station found in mission",
        }
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        use self::Error::*;

        match *self {
            Lua(ref err) => Some(err),
            Tcp(ref err) => Some(err),
            Json(ref err) => Some(err),
            Request(ref err) => Some(err),
            Base64Decode(ref err) => Some(err),
            Wav(ref err) => Some(err),
            Opus(ref err) => Some(err),
            _ => None,
        }
    }
}

impl From<::hlua51::LuaError> for Error {
    fn from(err: ::hlua51::LuaError) -> Self {
        Error::Lua(err)
    }
}

impl From<ArgsError> for Error {
    fn from(err: ArgsError) -> Self {
        Error::GetPluginArgs(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Tcp(err)
    }
}

impl From<serde_json::error::Error> for Error {
    fn from(err: serde_json::error::Error) -> Self {
        Error::Json(err)
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Request(err)
    }
}

impl From<base64::DecodeError> for Error {
    fn from(err: base64::DecodeError) -> Self {
        Error::Base64Decode(err)
    }
}

impl From<hound::Error> for Error {
    fn from(err: hound::Error) -> Self {
        Error::Wav(err)
    }
}

impl From<audiopus::Error> for Error {
    fn from(err: audiopus::Error) -> Self {
        Error::Opus(err)
    }
}
