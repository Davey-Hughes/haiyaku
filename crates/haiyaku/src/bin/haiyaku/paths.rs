//! Where the database and the cache are when no option says, by the XDG Base Directory Specification.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::Failure;

/// A lookup of environment variables: the process's in the program, a list of its own in a test.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// `$XDG_DATA_HOME/haiyaku/haiyaku.db`.
pub fn database(env: Env<'_>) -> Result<PathBuf, Failure> {
    let data = base(env, "XDG_DATA_HOME", ".local/share")
        .map_err(|why| Failure(format!("{why}: name the database with --db")))?;
    Ok(data.join("haiyaku").join("haiyaku.db"))
}

/// `$XDG_CACHE_HOME/haiyaku`, where a model's replies are kept.
pub fn cache(env: Env<'_>) -> Result<PathBuf, Failure> {
    let cache = base(env, "XDG_CACHE_HOME", ".cache")
        .map_err(|why| Failure(format!("{why}: run with --no-cache to keep no cache")))?;
    Ok(cache.join("haiyaku"))
}

/// The directory `$variable` names, or `$HOME/fallback` when it is unset, empty or not an absolute path, as
/// the specification says. A relative path would name another directory from each working directory, which
/// is why HOME, too, has to be absolute.
fn base(env: Env<'_>, variable: &str, fallback: &str) -> Result<PathBuf, String> {
    absolute(env(variable))
        .or_else(|| absolute(env("HOME")).map(|home| home.join(fallback)))
        .ok_or_else(|| format!("neither {variable} nor HOME is an absolute path"))
}

fn absolute(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use super::{cache, database};

    /// A lookup that knows `vars` and nothing else.
    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| vars.iter().find(|(var, _)| *var == name).map(|(_, value)| OsString::from(value))
    }

    const HOME: (&str, &str) = ("HOME", "/home/reader");

    #[test]
    fn unset_xdg_variables_give_way_to_home() {
        let env = env(&[HOME]);
        assert_eq!(database(&env).unwrap(), Path::new("/home/reader/.local/share/haiyaku/haiyaku.db"));
        assert_eq!(cache(&env).unwrap(), Path::new("/home/reader/.cache/haiyaku"));
    }

    #[test]
    fn empty_xdg_variables_give_way_to_home() {
        let env = env(&[HOME, ("XDG_DATA_HOME", ""), ("XDG_CACHE_HOME", "")]);
        assert_eq!(database(&env).unwrap(), Path::new("/home/reader/.local/share/haiyaku/haiyaku.db"));
        assert_eq!(cache(&env).unwrap(), Path::new("/home/reader/.cache/haiyaku"));
    }

    #[test]
    fn relative_xdg_variables_give_way_to_home() {
        let env = env(&[HOME, ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "./cache")]);
        assert_eq!(database(&env).unwrap(), Path::new("/home/reader/.local/share/haiyaku/haiyaku.db"));
        assert_eq!(cache(&env).unwrap(), Path::new("/home/reader/.cache/haiyaku"));
    }

    #[test]
    fn absolute_xdg_variables_are_used() {
        let housed = env(&[HOME, ("XDG_DATA_HOME", "/srv/data"), ("XDG_CACHE_HOME", "/var/cache/reader")]);
        assert_eq!(database(&housed).unwrap(), Path::new("/srv/data/haiyaku/haiyaku.db"));
        assert_eq!(cache(&housed).unwrap(), Path::new("/var/cache/reader/haiyaku"));
        // HOME is not needed then.
        let homeless = env(&[("XDG_DATA_HOME", "/srv/data"), ("XDG_CACHE_HOME", "/var/cache/reader")]);
        assert_eq!(database(&homeless).unwrap(), Path::new("/srv/data/haiyaku/haiyaku.db"));
        assert_eq!(cache(&homeless).unwrap(), Path::new("/var/cache/reader/haiyaku"));
    }

    #[test]
    fn without_an_absolute_home_there_is_no_default() {
        for vars in [&[][..], &[("HOME", "")], &[("HOME", "home/reader"), ("XDG_DATA_HOME", "data")]] {
            let message = database(&env(vars)).unwrap_err().to_string();
            assert!(message.contains("XDG_DATA_HOME") && message.contains("HOME"), "{message}");
            assert!(message.contains("--db"), "{message}");
            let message = cache(&env(vars)).unwrap_err().to_string();
            assert!(message.contains("XDG_CACHE_HOME") && message.contains("HOME"), "{message}");
            assert!(message.contains("--no-cache"), "{message}");
        }
    }
}
