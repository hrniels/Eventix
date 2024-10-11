use crate::objects::ICalTodo;
use std::{
    fs::{read_dir, File},
    io::BufReader,
};

mod objects;

fn main() {
    let dir = std::env::args().nth(1).unwrap();

    let mut todos = Vec::<ICalTodo>::new();
    for e in read_dir(dir).expect("Unable to read directory") {
        let buf = BufReader::new(File::open(e.unwrap().path()).unwrap());
        let reader = ical::IcalParser::new(buf);

        for line in reader {
            let cal = line.unwrap();
            for todo in &cal.todos {
                todos.push(todo.try_into().unwrap());
            }
        }
    }

    for t in &todos {
        println!("{:?}", t);
    }
}
