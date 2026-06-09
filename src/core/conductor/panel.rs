use crate::core::storage::Storage;
use std::io::Write;
use std::time::Duration;

pub fn run(session_id: String) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        print!("\x1b[2J\x1b[H");
        match render_panel(&session_id) {
            Ok(output) => print!("{}", output),
            Err(error) => {
                println!("Sub-session Details");
                println!();
                println!("Error: {}", error);
            }
        }
        std::io::stdout().flush()?;
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn render_panel(session_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let storage = Storage::open_default()?;
    storage.migrate()?;
    let children = storage.child_sessions(session_id)?;

    let mut output = String::from("Sub-session Details\n\n");
    for child in children {
        output.push_str(&format!(
            "{} {} {}\n",
            child.status.icon(),
            child.status.as_str(),
            child.title
        ));
    }
    Ok(output)
}
