use std::process;

pub trait UnwrapOrExit<T> {
    fn unwrap_or_exit(self, msg: &str) -> T;
}

impl<T, E: std::fmt::Display> UnwrapOrExit<T> for Result<T, E> {
    fn unwrap_or_exit(self, msg: &str) -> T {
        match self {
            Ok(value) => value,
            Err(e) => {
                eprintln!("Error: {}: {}", msg, e);
                process::exit(1);
            }
        }
    }
}

impl<T> UnwrapOrExit<T> for Option<T> {
    fn unwrap_or_exit(self, msg: &str) -> T {
        match self {
            Some(value) => value,
            None => {
                eprintln!("Error: {}", msg);
                process::exit(1);
            }
        }
    }
}
