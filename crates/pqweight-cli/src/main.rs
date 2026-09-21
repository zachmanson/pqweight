use std::io::Read;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("pqweight: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    let json = args.iter().any(|arg| arg == "--json");
    let positional: Vec<&String> = args.iter().filter(|arg| *arg != "--json").collect();
    let hex = match positional.as_slice() {
        [command, hex] if *command == "weight" => (*hex).clone(),
        [command] if *command == "weight" => read_stdin()?,
        _ => return Err("usage: pqweight weight [--json] [<hex>]".to_string()),
    };
    let bytes = decode_hex(hex.trim())?;
    let result = pqweight::transaction_weight(&bytes).map_err(|err| err.to_string())?;
    if json {
        Ok(format!(
            r#"{{"weight":{},"vsize":{},"stripped_size":{},"total_size":{}}}"#,
            result.weight, result.vsize, result.stripped_size, result.total_size
        ))
    } else {
        Ok(format!(
            "weight: {}\nvsize: {}\nstripped size: {}\ntotal size: {}",
            result.weight, result.vsize, result.stripped_size, result.total_size
        ))
    }
}

fn read_stdin() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|err| format!("could not read stdin: {err}"))?;
    Ok(input)
}

fn decode_hex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("invalid hex: odd number of digits".to_string());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| format!("invalid hex at position {i}"))
        })
        .collect()
}
