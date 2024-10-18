use once_cell::sync::Lazy;
use std::sync::Mutex;

mod item;
mod source;
mod store;

pub use item::CalItem;
pub use source::CalSource;
pub use store::CalStore;

pub type Id = u64;

pub fn generate_id() -> Id {
    static NEXT_ID: Lazy<Mutex<Id>> = Lazy::new(|| Mutex::new(0));
    let mut next = NEXT_ID.lock().unwrap();
    let res = *next + 1;
    *next += 1;
    res
}
