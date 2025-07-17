mod checks;

use bloodhound::results::*;
use bloodhound::system_access::NativeSystemAccess;
use checks::*;
use std::env;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    let cmd_name = Path::new(&args[0])
        .file_name()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default();

    let checker: Box<dyn Checker> = match cmd_name {
        "fips01000000" => Box::new(FIPS01000000Checker {}),
        "fips01010000" => Box::new(FIPS01010000Checker {}),
        "fips01020000" => Box::new(FIPS01020000Checker {}),
        &_ => {
            eprintln!("Command {cmd_name} is not supported.");
            return;
        }
    };

    // Check if the metadata subcommand is being called
    let get_metadata = env::args().nth(1).unwrap_or_default() == "metadata";
    let sac = NativeSystemAccess {};
    if get_metadata {
        let metadata = checker.metadata();
        println!("{metadata}");
    } else {
        let result = checker.execute(&sac);
        println!("{result}");
    }
}
