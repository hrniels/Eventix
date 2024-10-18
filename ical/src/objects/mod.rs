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

pub use calendar::{CalComponent, Calendar, Other};
pub use date::{CalDate, CalDateTime};
pub use event::CalEvent;
pub use item::CalItem;
pub use recur::CalRRule;
pub use source::CalSource;
pub use status::CalStatus;
pub use store::CalStore;
pub use todo::CalTodo;

pub type Id = u64;

pub fn generate_id() -> Id {
    static NEXT_ID: Lazy<Mutex<Id>> = Lazy::new(|| Mutex::new(0));
    let mut next = NEXT_ID.lock().unwrap();
    let res = *next + 1;
    *next += 1;
    res
}
