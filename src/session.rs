//! The tabs that come back at launch. Only the address, title and whether the
//! tab is pinned are kept; a restored tab loads nothing until you open it.
//! Private tabs are never written down.

use crate::store;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
pub struct Session {
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub current: usize,
}

impl Session {
    fn file() -> PathBuf {
        store::data_dir().join("session.json")
    }

    pub fn load() -> Session {
        store::load(&Self::file())
    }

    pub fn save(&self) {
        if let Err(err) = store::save(&Self::file(), self) {
            eprintln!("wisp: couldn't save open tabs: {err}");
        }
    }
}
