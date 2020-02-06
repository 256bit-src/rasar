use std::{convert::From, error, fmt, io, num::ParseIntError};

#[derive(Debug)]
pub enum Error {
	IoError(io::Error),
	ParseIntError(ParseIntError),
	JsonError(serde_json::Error),
	GlobError(glob::GlobError),
}

impl fmt::Display for Error {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match self {
			Error::IoError(ref err) => write!(f, "IO Error: {}", err),
			Error::ParseIntError(ref err) => write!(f, "Error parsing int: {}", err),
			Error::JsonError(ref err) => write!(f, "Error parsing JSON: {}", err),
			Error::GlobError(ref err) => write!(f, "Error parsing glob: {}", err),
		}
	}
}

impl error::Error for Error {
	fn source(&self) -> Option<&(dyn error::Error + 'static)> {
		match self {
			Error::IoError(ref err) => Some(err),
			Error::ParseIntError(ref err) => Some(err),
			Error::JsonError(ref err) => Some(err),
			Error::GlobError(ref err) => Some(err),
		}
	}
}

impl From<io::Error> for Error {
	fn from(err: io::Error) -> Self {
		Error::IoError(err)
	}
}

impl From<serde_json::Error> for Error {
	fn from(err: serde_json::Error) -> Self {
		Error::JsonError(err)
	}
}

impl From<glob::GlobError> for Error {
	fn from(err: glob::GlobError) -> Self {
		Error::GlobError(err)
	}
}

impl From<ParseIntError> for Error {
	fn from(err: ParseIntError) -> Self {
		Error::ParseIntError(err)
	}
}
