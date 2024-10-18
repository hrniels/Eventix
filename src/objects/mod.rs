use once_cell::sync::Lazy;
use std::sync::Mutex;

mod calendar;
mod date;
mod event;
mod item;
mod recur;
mod source;
mod status;
mod store;
mod todo;

pub use date::CalDate;
pub use event::Event;
pub use item::CalItem;
pub use recur::RecurrenceRule;
pub use source::CalSource;
pub use status::Status;
pub use store::CalStore;
pub use todo::Todo;

pub type Id = u64;

pub fn generate_id() -> Id {
    static NEXT_ID: Lazy<Mutex<Id>> = Lazy::new(|| Mutex::new(0));
    let mut next = NEXT_ID.lock().unwrap();
    let res = *next + 1;
    *next += 1;
    res
}
