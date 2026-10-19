//! Isolated lifecycle test executable; never installed as a plugin.
use herdrkit::{
    Invocation,
    service::{Handler, Service},
};
use serde_json::{Value, json};

#[derive(Debug, Default)]
struct Counter(u64);
impl Handler for Counter {
    fn request(&mut self, _: Value) -> Result<Value, String> {
        self.0 += 1;
        Ok(self.status())
    }
    fn tick(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn status(&self) -> Value {
        json!({"pid": std::process::id(), "requests": self.0})
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let invocation = Invocation::load()?;
    let service = Service::new(&invocation, "fixture")?;
    if service.is_worker() {
        service.run(Counter::default())?;
    } else {
        match std::env::args().nth(1).as_deref() {
            Some("stop") => service.shutdown()?,
            Some("wake") => println!("{}", service.wake(Value::Null)?),
            _ => {
                service.ensure()?;
                println!("{}", service.status()?);
            }
        }
    }
    Ok(())
}
